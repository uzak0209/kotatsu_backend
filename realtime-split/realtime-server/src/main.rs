use anyhow::{anyhow, Context, Result};
use kotatsu_proto::controlplane::v1::{
    control_plane_server::{ControlPlane, ControlPlaneServer},
    CreateRoomRequest, CreateRoomResponse, GetRoomRequest, GetRoomResponse, IssueJoinTicketRequest,
    IssueJoinTicketResponse, RoomPlayer,
};
use quinn::{Connection, Endpoint, RecvStream, SendStream, ServerConfig};
use rcgen::generate_simple_self_signed;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, Mutex};
use tonic::{Request, Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

const ROOM_MAX_PLAYERS: usize = 4;
const TOKEN_TTL_SECS: u64 = 60 * 60;
const PARAM_DEFAULT_LEVEL: u8 = 2;
const PARAM_MIN_LEVEL: u8 = 1;
const PARAM_MAX_LEVEL_THREE_STAGE: u8 = 3;
const PARAM_MAX_LEVEL_FRICTION: u8 = 2;
const PARAM_CHANGE_COOLDOWN_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlayerParams {
    gravity: u8,
    friction: u8,
    speed: u8,
}

impl Default for PlayerParams {
    fn default() -> Self {
        Self {
            gravity: PARAM_DEFAULT_LEVEL,
            friction: PARAM_DEFAULT_LEVEL,
            speed: PARAM_DEFAULT_LEVEL,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParamKind {
    Gravity,
    Friction,
    Speed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParamDirection {
    Increase,
    Decrease,
}

#[derive(Debug, Clone)]
struct ParamMutation {
    params: PlayerParams,
    next_param_change_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParamUpdateError {
    CooldownActive { next_allowed_at_unix: u64 },
    OutOfRange,
}

impl std::fmt::Display for ParamUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CooldownActive {
                next_allowed_at_unix,
            } => {
                write!(f, "cooldown_active:{next_allowed_at_unix}")
            }
            Self::OutOfRange => write!(f, "out_of_range"),
        }
    }
}

impl std::error::Error for ParamUpdateError {}

#[derive(Debug, Clone)]
struct Ticket {
    match_id: String,
    player_id: String,
    display_name: String,
    expires_at_unix: u64,
}

#[derive(Debug)]
struct PlayerHandle {
    display_name: String,
    params: PlayerParams,
    next_param_change_at_unix: u64,
    reliable_tx: mpsc::Sender<ServerReliable>,
    connection: Option<Connection>,
}

#[derive(Debug)]
struct MatchRoom {
    players: HashMap<String, PlayerHandle>,
}

impl MatchRoom {
    fn new() -> Self {
        Self {
            players: HashMap::new(),
        }
    }
}

#[derive(Debug, Default)]
struct CoreState {
    matches: HashMap<String, MatchRoom>,
    tickets: HashMap<String, Ticket>,
}

#[derive(Clone)]
struct AppState {
    core: Arc<Mutex<CoreState>>,
    quic_public_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ClientReliable {
    Join { token: String },
    ParamChange {
        seq: u64,
        param: ParamKind,
        direction: ParamDirection,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ServerReliable {
    JoinOk {
        match_id: String,
        player_id: String,
        params: PlayerParams,
        server_time_ms: u64,
    },
    ParamApplied {
        from_player_id: String,
        seq: u64,
        params: PlayerParams,
        next_param_change_at_unix: u64,
        server_time_ms: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ClientDatagram {
    Pos { seq: u64, x: f32, y: f32, vx: f32, vy: f32 },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ServerDatagram {
    Pos {
        player_id: String,
        seq: u64,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        server_time_ms: u64,
    },
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

fn param_level_bounds(param: ParamKind) -> (u8, u8) {
    match param {
        ParamKind::Gravity | ParamKind::Speed => (PARAM_MIN_LEVEL, PARAM_MAX_LEVEL_THREE_STAGE),
        ParamKind::Friction => (PARAM_MIN_LEVEL, PARAM_MAX_LEVEL_FRICTION),
    }
}

fn apply_param_change(
    params: &PlayerParams,
    next_param_change_at_unix: u64,
    param: ParamKind,
    direction: ParamDirection,
    now_unix: u64,
) -> std::result::Result<ParamMutation, ParamUpdateError> {
    if now_unix < next_param_change_at_unix {
        return Err(ParamUpdateError::CooldownActive {
            next_allowed_at_unix: next_param_change_at_unix,
        });
    }

    let mut next = params.clone();
    let slot = match param {
        ParamKind::Gravity => &mut next.gravity,
        ParamKind::Friction => &mut next.friction,
        ParamKind::Speed => &mut next.speed,
    };

    let candidate = match direction {
        ParamDirection::Increase => slot.saturating_add(1),
        ParamDirection::Decrease => slot.saturating_sub(1),
    };

    let (min_level, max_level) = param_level_bounds(param);
    if !(min_level..=max_level).contains(&candidate) {
        return Err(ParamUpdateError::OutOfRange);
    }

    *slot = candidate;

    Ok(ParamMutation {
        params: next,
        next_param_change_at_unix: now_unix + PARAM_CHANGE_COOLDOWN_SECS,
    })
}

async fn read_json_line<T: for<'de> Deserialize<'de>>(recv: &mut RecvStream, buf: &mut Vec<u8>) -> Result<T> {
    loop {
        if let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line = buf.drain(..=pos).collect::<Vec<u8>>();
            let line = &line[..line.len() - 1];
            if line.is_empty() {
                continue;
            }
            let msg = serde_json::from_slice::<T>(line).context("parse json line")?;
            return Ok(msg);
        }
        let chunk = recv.read_chunk(4096, true).await.context("read reliable chunk")?;
        match chunk {
            Some(c) => buf.extend_from_slice(&c.bytes),
            None => return Err(anyhow!("stream closed")),
        }
    }
}

async fn write_json_line<T: Serialize>(send: &mut SendStream, msg: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(msg)?;
    bytes.push(b'\n');
    send.write_all(&bytes).await?;
    Ok(())
}

async fn remove_player(st: &AppState, match_id: &str, player_id: &str) {
    let mut core = st.core.lock().await;
    if let Some(room) = core.matches.get_mut(match_id) {
        room.players.remove(player_id);
    }
}

async fn update_player_params(
    st: &AppState,
    match_id: &str,
    player_id: &str,
    param: ParamKind,
    direction: ParamDirection,
) -> Result<ParamMutation> {
    let mut core = st.core.lock().await;
    let room = core
        .matches
        .get_mut(match_id)
        .ok_or_else(|| anyhow!("match_not_found"))?;
    let player = room
        .players
        .get_mut(player_id)
        .ok_or_else(|| anyhow!("player_not_found"))?;
    let updated = apply_param_change(
        &player.params,
        player.next_param_change_at_unix,
        param,
        direction,
        now_unix(),
    )?;
    player.params = updated.params.clone();
    player.next_param_change_at_unix = updated.next_param_change_at_unix;
    Ok(updated)
}

async fn broadcast_reliable(st: &AppState, match_id: &str, msg: ServerReliable) {
    let senders: Vec<mpsc::Sender<ServerReliable>> = {
        let core = st.core.lock().await;
        let Some(room) = core.matches.get(match_id) else {
            return;
        };
        room.players.values().map(|p| p.reliable_tx.clone()).collect()
    };

    for tx in senders {
        let _ = tx.send(msg.clone()).await;
    }
}

async fn broadcast_datagram(st: &AppState, match_id: &str, sender_player_id: &str, payload: Vec<u8>) {
    let conns: Vec<Connection> = {
        let core = st.core.lock().await;
        let Some(room) = core.matches.get(match_id) else {
            return;
        };
        room.players
            .iter()
            .filter_map(|(pid, p)| {
                if pid == sender_player_id { return None; }
                p.connection.clone()
            })
            .collect()
    };

    for c in conns {
        let _ = c.send_datagram(payload.clone().into());
    }
}

async fn consume_ticket(st: &AppState, token: &str) -> Result<(String, String, String)> {
    let mut core = st.core.lock().await;
    let t = core
        .tickets
        .remove(token)
        .ok_or_else(|| anyhow!("invalid_token"))?;
    if t.expires_at_unix < now_unix() {
        return Err(anyhow!("token_expired"));
    }
    Ok((t.match_id, t.player_id, t.display_name))
}

async fn register_player(
    st: &AppState,
    match_id: &str,
    player_id: &str,
    display_name: String,
    conn: &Connection,
    reliable_tx: mpsc::Sender<ServerReliable>,
) -> Result<PlayerParams> {
    let mut core = st.core.lock().await;
    let room = core
        .matches
        .get_mut(match_id)
        .ok_or_else(|| anyhow!("match_not_found"))?;

    if room.players.len() >= ROOM_MAX_PLAYERS && !room.players.contains_key(player_id) {
        return Err(anyhow!("match_full"));
    }

    let params = room
        .players
        .get(player_id)
        .map(|p| p.params.clone())
        .unwrap_or_default();

    room.players.insert(
        player_id.to_string(),
        PlayerHandle {
            display_name,
            params: params.clone(),
            next_param_change_at_unix: 0,
            reliable_tx,
            connection: Some(conn.clone()),
        },
    );

    Ok(params)
}

async fn handle_quic_connection(st: AppState, conn: Connection) {
    let (mut send, mut recv) = match conn.accept_bi().await {
        Ok(v) => v,
        Err(e) => {
            warn!("accept_bi failed: {e}");
            return;
        }
    };

    let mut buf = Vec::with_capacity(4096);
    let token = match read_json_line::<ClientReliable>(&mut recv, &mut buf).await {
        Ok(ClientReliable::Join { token }) => token,
        Ok(_) => {
            let _ = write_json_line(
                &mut send,
                &ServerReliable::Error {
                    code: "invalid_first_message".into(),
                    message: "first reliable message must be join".into(),
                },
            )
            .await;
            return;
        }
        Err(e) => {
            warn!("read join failed: {e:#}");
            return;
        }
    };

    let (match_id, player_id, display_name) = match consume_ticket(&st, &token).await {
        Ok(v) => v,
        Err(e) => {
            let _ = write_json_line(
                &mut send,
                &ServerReliable::Error {
                    code: "auth_failed".into(),
                    message: e.to_string(),
                },
            )
            .await;
            return;
        }
    };

    let (tx, mut rx) = mpsc::channel::<ServerReliable>(1024);
    let params = match register_player(
        &st,
        &match_id,
        &player_id,
        display_name,
        &conn,
        tx.clone(),
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            let _ = write_json_line(
                &mut send,
                &ServerReliable::Error {
                    code: "join_failed".into(),
                    message: e.to_string(),
                },
            )
            .await;
            return;
        }
    };

    if write_json_line(
        &mut send,
        &ServerReliable::JoinOk {
            match_id: match_id.clone(),
            player_id: player_id.clone(),
            params,
            server_time_ms: now_ms(),
        },
    )
    .await
    .is_err()
    {
        remove_player(&st, &match_id, &player_id).await;
        return;
    }

    let mut send_for_task = send;
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write_json_line(&mut send_for_task, &msg).await.is_err() {
                break;
            }
        }
    });

    let st_for_dgram = st.clone();
    let mid_for_dgram = match_id.clone();
    let pid_for_dgram = player_id.clone();
    let conn_for_dgram = conn.clone();
    let datagram_task = tokio::spawn(async move {
        while let Ok(bytes) = conn_for_dgram.read_datagram().await {
            let parsed = serde_json::from_slice::<ClientDatagram>(&bytes);
            let ClientDatagram::Pos { seq, x, y, vx, vy } = match parsed {
                Ok(v) => v,
                Err(_) => continue,
            };

            let out = ServerDatagram::Pos {
                player_id: pid_for_dgram.clone(),
                seq,
                x,
                y,
                vx,
                vy,
                server_time_ms: now_ms(),
            };
            if let Ok(payload) = serde_json::to_vec(&out) {
                broadcast_datagram(&st_for_dgram, &mid_for_dgram, &pid_for_dgram, payload).await;
            }
        }
    });

    loop {
        let msg = read_json_line::<ClientReliable>(&mut recv, &mut buf).await;
        let msg = match msg {
            Ok(v) => v,
            Err(_) => break,
        };

        if let ClientReliable::ParamChange {
            seq,
            param,
            direction,
        } = msg
        {
            let updated = match update_player_params(&st, &match_id, &player_id, param, direction).await {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx
                        .send(ServerReliable::Error {
                            code: "param_update_failed".into(),
                            message: e.to_string(),
                        })
                        .await;
                    continue;
                }
            };

            broadcast_reliable(
                &st,
                &match_id,
                ServerReliable::ParamApplied {
                    from_player_id: player_id.clone(),
                    seq,
                    params: updated.params,
                    next_param_change_at_unix: updated.next_param_change_at_unix,
                    server_time_ms: now_ms(),
                },
            )
            .await;
        }
    }

    datagram_task.abort();
    writer_task.abort();
    remove_player(&st, &match_id, &player_id).await;
    info!("player disconnected: match={match_id} player={player_id}");
}

#[derive(Clone)]
struct ControlPlaneSvc {
    st: AppState,
}

#[tonic::async_trait]
impl ControlPlane for ControlPlaneSvc {
    async fn create_room(
        &self,
        _request: Request<CreateRoomRequest>,
    ) -> Result<Response<CreateRoomResponse>, Status> {
        let match_id = format!("m_{}", Uuid::new_v4().simple());
        let mut core = self.st.core.lock().await;
        core.matches.insert(match_id.clone(), MatchRoom::new());

        Ok(Response::new(CreateRoomResponse {
            match_id,
            max_players: ROOM_MAX_PLAYERS as u32,
        }))
    }

    async fn issue_join_ticket(
        &self,
        request: Request<IssueJoinTicketRequest>,
    ) -> Result<Response<IssueJoinTicketResponse>, Status> {
        let req = request.into_inner();
        let display_name = if req.display_name.trim().is_empty() {
            "player".to_string()
        } else {
            req.display_name
        };

        let mut core = self.st.core.lock().await;
        let room = core
            .matches
            .get_mut(&req.match_id)
            .ok_or_else(|| Status::not_found("match_not_found"))?;

        if room.players.len() >= ROOM_MAX_PLAYERS {
            return Err(Status::failed_precondition("match_full"));
        }

        let player_id = format!("p_{}", Uuid::new_v4().simple());
        let token = Uuid::new_v4().to_string();
        let expires = now_unix() + TOKEN_TTL_SECS;

        core.tickets.insert(
            token.clone(),
            Ticket {
                match_id: req.match_id.clone(),
                player_id: player_id.clone(),
                display_name,
                expires_at_unix: expires,
            },
        );

        Ok(Response::new(IssueJoinTicketResponse {
            match_id: req.match_id,
            player_id,
            token,
            token_expires_at_unix: expires,
            quic_url: self.st.quic_public_url.clone(),
        }))
    }

    async fn get_room(
        &self,
        request: Request<GetRoomRequest>,
    ) -> Result<Response<GetRoomResponse>, Status> {
        let req = request.into_inner();
        let core = self.st.core.lock().await;
        let room = core
            .matches
            .get(&req.match_id)
            .ok_or_else(|| Status::not_found("match_not_found"))?;

        let players = room
            .players
            .iter()
            .map(|(id, p)| RoomPlayer {
                player_id: id.clone(),
                display_name: p.display_name.clone(),
                gravity: u32::from(p.params.gravity),
                friction: u32::from(p.params.friction),
                speed: u32::from(p.params.speed),
                next_param_change_at_unix: p.next_param_change_at_unix,
            })
            .collect();

        Ok(Response::new(GetRoomResponse {
            match_id: req.match_id,
            max_players: ROOM_MAX_PLAYERS as u32,
            players,
        }))
    }
}

fn build_quic_server(addr: SocketAddr) -> Result<Endpoint> {
    let cert = generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])?;
    let cert_der = cert.cert.der().to_vec();
    let key_der = cert.key_pair.serialize_der();

    let certs = vec![CertificateDer::from(cert_der)];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));
    let server_config = ServerConfig::with_single_cert(certs, key)?;

    Ok(Endpoint::server(server_config, addr)?)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let grpc_addr: SocketAddr = std::env::var("GRPC_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".into())
        .parse()?;
    let quic_bind_addr: SocketAddr = std::env::var("QUIC_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:4433".into())
        .parse()?;
    let quic_public_url = std::env::var("QUIC_PUBLIC_URL").unwrap_or_else(|_| {
        let host = std::env::var("PUBLIC_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("QUIC_PORT").unwrap_or_else(|_| "4433".into());
        format!("quic://{host}:{port}")
    });

    let st = AppState {
        core: Arc::new(Mutex::new(CoreState::default())),
        quic_public_url,
    };

    let quic_endpoint = build_quic_server(quic_bind_addr)?;
    info!("realtime QUIC listening on {quic_bind_addr}");

    let quic_state = st.clone();
    tokio::spawn(async move {
        loop {
            let Some(incoming) = quic_endpoint.accept().await else {
                break;
            };
            let st_clone = quic_state.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(conn) => handle_quic_connection(st_clone, conn).await,
                    Err(e) => warn!("quic accept failed: {e:#}"),
                }
            });
        }
    });

    let svc = ControlPlaneSvc { st };
    info!("control gRPC listening on {grpc_addr}");

    tonic::transport::Server::builder()
        .add_service(ControlPlaneServer::new(svc))
        .serve(grpc_addr)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_change_updates_one_step_only() {
        let params = PlayerParams::default();
        let updated = apply_param_change(
            &params,
            0,
            ParamKind::Gravity,
            ParamDirection::Increase,
            100,
        )
        .expect("should update");
        assert_eq!(updated.params.gravity, 3);
        assert_eq!(updated.params.friction, 2);
        assert_eq!(updated.params.speed, 2);
        assert_eq!(updated.next_param_change_at_unix, 130);
    }

    #[test]
    fn friction_can_only_toggle_between_off_and_on() {
        let params = PlayerParams::default();
        let updated = apply_param_change(
            &params,
            0,
            ParamKind::Friction,
            ParamDirection::Decrease,
            100,
        )
        .expect("should update");
        assert_eq!(updated.params.friction, 1);

        let err = apply_param_change(
            &params,
            0,
            ParamKind::Friction,
            ParamDirection::Increase,
            100,
        )
        .expect_err("must reject");
        assert_eq!(err, ParamUpdateError::OutOfRange);
    }

    #[test]
    fn param_change_respects_range_limit() {
        let params = PlayerParams {
            gravity: 3,
            friction: 2,
            speed: 2,
        };
        let err = apply_param_change(
            &params,
            0,
            ParamKind::Gravity,
            ParamDirection::Increase,
            100,
        )
        .expect_err("must reject");
        assert_eq!(err, ParamUpdateError::OutOfRange);
    }

    #[test]
    fn param_change_respects_cooldown() {
        let params = PlayerParams::default();
        let err = apply_param_change(
            &params,
            120,
            ParamKind::Speed,
            ParamDirection::Decrease,
            100,
        )
        .expect_err("must reject");
        assert_eq!(
            err,
            ParamUpdateError::CooldownActive {
                next_allowed_at_unix: 120
            }
        );
    }

    #[tokio::test]
    async fn update_player_params_persists_internal_state() {
        let player_id = "p_test".to_string();
        let match_id = "m_test".to_string();
        let st = AppState {
            core: Arc::new(Mutex::new(CoreState {
                matches: HashMap::from([(
                    match_id.clone(),
                    MatchRoom {
                        players: HashMap::from([(
                            player_id.clone(),
                            PlayerHandle {
                                display_name: "tester".into(),
                                params: PlayerParams::default(),
                                next_param_change_at_unix: 0,
                                reliable_tx: mpsc::channel(1).0,
                                connection: None,
                            },
                        )]),
                    },
                )]),
                tickets: HashMap::new(),
            })),
            quic_public_url: "quic://127.0.0.1:4433".into(),
        };

        let updated = update_player_params(
            &st,
            &match_id,
            &player_id,
            ParamKind::Gravity,
            ParamDirection::Increase,
        )
        .await
        .expect("update should succeed");

        let core = st.core.lock().await;
        let room = core.matches.get(&match_id).expect("room should exist");
        let player = room.players.get(&player_id).expect("player should exist");
        assert_eq!(updated.params.gravity, 3);
        assert_eq!(player.params.gravity, 3);
        assert_eq!(player.params.friction, 2);
        assert_eq!(player.params.speed, 2);
        assert_eq!(
            player.next_param_change_at_unix,
            updated.next_param_change_at_unix
        );
    }

    #[tokio::test]
    async fn update_player_params_allows_friction_toggle_to_one() {
        let player_id = "p_test".to_string();
        let match_id = "m_test".to_string();
        let st = AppState {
            core: Arc::new(Mutex::new(CoreState {
                matches: HashMap::from([(
                    match_id.clone(),
                    MatchRoom {
                        players: HashMap::from([(
                            player_id.clone(),
                            PlayerHandle {
                                display_name: "tester".into(),
                                params: PlayerParams::default(),
                                next_param_change_at_unix: 0,
                                reliable_tx: mpsc::channel(1).0,
                                connection: None,
                            },
                        )]),
                    },
                )]),
                tickets: HashMap::new(),
            })),
            quic_public_url: "quic://127.0.0.1:4433".into(),
        };

        let updated = update_player_params(
            &st,
            &match_id,
            &player_id,
            ParamKind::Friction,
            ParamDirection::Decrease,
        )
        .await
        .expect("update should succeed");

        let core = st.core.lock().await;
        let room = core.matches.get(&match_id).expect("room should exist");
        let player = room.players.get(&player_id).expect("player should exist");
        assert_eq!(updated.params.friction, 1);
        assert_eq!(player.params.friction, 1);
    }
}
