use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
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
use tokio::{
    sync::{mpsc, Mutex},
};
use tracing::{error, info, warn};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

const ROOM_MAX_PLAYERS: usize = 4;
const TOKEN_TTL_SECS: u64 = 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct PlayerParams {
    gravity: f32,
    friction: f32,
    speed: f32,
}

impl Default for PlayerParams {
    fn default() -> Self {
        Self {
            gravity: 1.0,
            friction: 1.0,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
struct Ticket {
    match_id: String,
    player_id: String,
    expires_at_unix: u64,
}

#[derive(Debug)]
struct PlayerHandle {
    params: PlayerParams,
    reliable_tx: mpsc::Sender<ServerReliable>,
    connection: Connection,
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
    quic_addr: String,
}

#[derive(Debug, Deserialize, ToSchema)]
struct CreateMatchReq {}

#[derive(Debug, Serialize, ToSchema)]
struct CreateMatchRes {
    match_id: String,
    max_players: usize,
}

#[derive(Debug, Deserialize, ToSchema)]
struct JoinMatchReq {
    display_name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
struct JoinMatchRes {
    match_id: String,
    player_id: String,
    token: String,
    quic_url: String,
    token_expires_at_unix: u64,
}

#[derive(Debug, Serialize, ToSchema)]
struct HealthRes {
    ok: bool,
}

#[derive(Debug, Serialize, ToSchema)]
struct ErrorRes {
    error: String,
}

#[derive(OpenApi)]
#[openapi(
    paths(health, create_match, join_match),
    components(schemas(
        PlayerParams,
        CreateMatchReq,
        CreateMatchRes,
        JoinMatchReq,
        JoinMatchRes,
        HealthRes,
        ErrorRes
    )),
    tags(
        (name = "matchmaking", description = "Matchmaking API")
    )
)]
struct ApiDoc;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ClientReliable {
    Join { token: String },
    ParamSet { seq: u64, gravity: f32, friction: f32, speed: f32 },
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
    Pos {
        seq: u64,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
    },
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

#[utoipa::path(
    get,
    path = "/health",
    tag = "matchmaking",
    responses(
        (status = 200, description = "Health status", body = HealthRes)
    )
)]
async fn health() -> impl IntoResponse {
    Json(HealthRes { ok: true })
}

#[utoipa::path(
    post,
    path = "/v1/matches",
    tag = "matchmaking",
    request_body = CreateMatchReq,
    responses(
        (status = 200, description = "Match created", body = CreateMatchRes)
    )
)]
async fn create_match(State(st): State<AppState>, Json(_req): Json<CreateMatchReq>) -> impl IntoResponse {
    let match_id = format!("m_{}", Uuid::new_v4().simple());
    let mut core = st.core.lock().await;
    core.matches
        .insert(match_id.clone(), MatchRoom::new());
    (
        StatusCode::OK,
        Json(CreateMatchRes {
            match_id,
            max_players: ROOM_MAX_PLAYERS,
        }),
    )
}

#[utoipa::path(
    post,
    path = "/v1/matches/{match_id}/join",
    tag = "matchmaking",
    request_body = JoinMatchReq,
    params(
        ("match_id" = String, Path, description = "Match ID")
    ),
    responses(
        (status = 200, description = "Join token issued", body = JoinMatchRes),
        (status = 404, description = "Match not found", body = ErrorRes),
        (status = 409, description = "Match full", body = ErrorRes)
    )
)]
async fn join_match(
    State(st): State<AppState>,
    Path(match_id): Path<String>,
    Json(req): Json<JoinMatchReq>,
) -> impl IntoResponse {
    let mut core = st.core.lock().await;
    let room = match core.matches.get_mut(&match_id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorRes {
                    error: "match_not_found".into(),
                }),
            )
                .into_response()
        }
    };

    if room.players.len() >= ROOM_MAX_PLAYERS {
        return (
            StatusCode::CONFLICT,
            Json(ErrorRes {
                error: "match_full".into(),
            }),
        )
            .into_response();
    }

    let player_id = format!("p_{}", Uuid::new_v4().simple());
    let _display_name = req.display_name.unwrap_or_else(|| player_id.clone());
    let token = Uuid::new_v4().to_string();
    let expires = now_unix() + TOKEN_TTL_SECS;

    core.tickets.insert(
        token.clone(),
        Ticket {
            match_id: match_id.clone(),
            player_id: player_id.clone(),
            expires_at_unix: expires,
        },
    );

    (
        StatusCode::OK,
        Json(JoinMatchRes {
            match_id,
            player_id,
            token,
            quic_url: st.quic_addr.clone(),
            token_expires_at_unix: expires,
        }),
    )
        .into_response()
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

async fn verify_ticket(st: &AppState, token: &str) -> Result<(String, String)> {
    let mut core = st.core.lock().await;
    let t = core
        .tickets
        .remove(token)
        .ok_or_else(|| anyhow!("invalid_token"))?;

    if t.expires_at_unix < now_unix() {
        return Err(anyhow!("token_expired"));
    }
    Ok((t.match_id, t.player_id))
}

async fn register_player(
    st: &AppState,
    match_id: &str,
    player_id: &str,
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
            params: params.clone(),
            reliable_tx,
            connection: conn.clone(),
        },
    );
    Ok(params)
}

async fn remove_player(st: &AppState, match_id: &str, player_id: &str) {
    let mut core = st.core.lock().await;
    if let Some(room) = core.matches.get_mut(match_id) {
        room.players.remove(player_id);
    }
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

async fn update_player_params(
    st: &AppState,
    match_id: &str,
    player_id: &str,
    params: PlayerParams,
) -> Result<()> {
    let mut core = st.core.lock().await;
    let room = core
        .matches
        .get_mut(match_id)
        .ok_or_else(|| anyhow!("match_not_found"))?;
    let p = room
        .players
        .get_mut(player_id)
        .ok_or_else(|| anyhow!("player_not_found"))?;
    p.params = params;
    Ok(())
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
                if pid == sender_player_id {
                    None
                } else {
                    Some(p.connection.clone())
                }
            })
            .collect()
    };

    for c in conns {
        let _ = c.send_datagram(payload.clone().into());
    }
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
    let join = match read_json_line::<ClientReliable>(&mut recv, &mut buf).await {
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
            warn!("failed to read join: {e:#}");
            return;
        }
    };

    let (match_id, player_id) = match verify_ticket(&st, &join).await {
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
    let params = match register_player(&st, &match_id, &player_id, &conn, tx.clone()).await {
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

        if let ClientReliable::ParamSet {
            seq,
            gravity,
            friction,
            speed,
        } = msg
        {
            let params = PlayerParams {
                gravity,
                friction,
                speed,
            };

            if let Err(e) = update_player_params(&st, &match_id, &player_id, params.clone()).await {
                let _ = tx
                    .send(ServerReliable::Error {
                        code: "param_update_failed".into(),
                        message: e.to_string(),
                    })
                    .await;
                continue;
            }

            broadcast_reliable(
                &st,
                &match_id,
                ServerReliable::ParamApplied {
                    from_player_id: player_id.clone(),
                    seq,
                    params,
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

    let api_addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let quic_addr: SocketAddr = "0.0.0.0:4433".parse()?;

    let st = AppState {
        core: Arc::new(Mutex::new(CoreState::default())),
        quic_addr: format!("quic://{}", quic_addr),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/matches", post(create_match))
        .route("/v1/matches/:match_id/join", post(join_match))
        .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()))
        .with_state(st.clone());

    let listener = tokio::net::TcpListener::bind(api_addr).await?;
    info!("matchmaking api listening on {api_addr}");

    let quic_endpoint = build_quic_server(quic_addr)?;
    info!("quic realtime listening on {quic_addr}");

    let api_task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("api server error: {e:#}");
        }
    });

    loop {
        let Some(incoming) = quic_endpoint.accept().await else {
            break;
        };
        let st_clone = st.clone();
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => handle_quic_connection(st_clone, conn).await,
                Err(e) => warn!("quic accept failed: {e:#}"),
            }
        });
    }

    api_task.abort();
    Ok(())
}
