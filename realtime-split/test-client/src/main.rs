use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{lookup_host, UdpSocket},
    sync::{mpsc, Mutex},
    time::sleep,
};
use url::Url;

const DEFAULT_TICK_MS: u64 = 32;
const DEFAULT_TICKS: u64 = 90;
const SNAPSHOT_LINGER_MS: u64 = 1500;
const PKT_RELIABLE: u8 = 0x01;
const PKT_UNRELIABLE: u8 = 0x02;

#[derive(Debug, Deserialize)]
struct CreateMatchRes {
    match_id: String,
    max_players: u32,
}

#[derive(Debug, Deserialize, Clone)]
struct JoinMatchRes {
    player_id: String,
    token: String,
    udp_url: String,
}

#[derive(Debug, Deserialize)]
struct GetMatchRes {
    match_id: String,
    max_players: u32,
    started_at_unix: u64,
    players: Vec<RoomPlayerRes>,
}

#[derive(Debug, Deserialize)]
struct RoomPlayerRes {
    player_id: String,
    display_name: String,
    gravity: u32,
    friction: u32,
    speed: u32,
    next_param_change_at_unix: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ParamKind {
    Gravity,
    Friction,
    Speed,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ParamDirection {
    Increase,
    Decrease,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct PlayerParams {
    gravity: u8,
    friction: u8,
    speed: u8,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ClientReliable {
    Join {
        token: String,
    },
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
    MatchStarted {
        match_id: String,
        started_at_unix: u64,
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

#[derive(Debug, Clone, Default)]
struct ClientStats {
    param_applied_count: usize,
    datagram_count: usize,
    datagram_from_players: HashSet<String>,
    join_params: Option<PlayerParams>,
    observed_param_state: Option<PlayerParams>,
    cooldown_error_seen: bool,
    next_param_change_at_unix: Option<u64>,
}

fn encode_packet<T: Serialize>(pkt_type: u8, msg: &T) -> Result<Vec<u8>> {
    let mut packet = vec![pkt_type];
    packet.extend_from_slice(&serde_json::to_vec(msg)?);
    Ok(packet)
}

fn parse_reliable(packet: &[u8]) -> Option<ServerReliable> {
    if packet.first().copied() != Some(PKT_RELIABLE) {
        return None;
    }
    serde_json::from_slice(&packet[1..]).ok()
}

fn parse_datagram(packet: &[u8]) -> Option<ServerDatagram> {
    if packet.first().copied() != Some(PKT_UNRELIABLE) {
        return None;
    }
    serde_json::from_slice(&packet[1..]).ok()
}

async fn connect_udp_socket(udp_url: &str) -> Result<Arc<UdpSocket>> {
    let url = Url::parse(udp_url).context("parse udp_url")?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("udp_url host missing"))?;
    let port = url.port().ok_or_else(|| anyhow!("udp_url port missing"))?;
    let remote_addr = lookup_host((host, port))
        .await
        .context("resolve remote host")?
        .next()
        .ok_or_else(|| anyhow!("no remote addr resolved"))?;

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(remote_addr).await?;
    Ok(Arc::new(socket))
}

async fn send_reliable<T: Serialize>(socket: &UdpSocket, msg: &T) -> Result<()> {
    let packet = encode_packet(PKT_RELIABLE, msg)?;
    socket.send(&packet).await?;
    Ok(())
}

async fn send_datagram<T: Serialize>(socket: &UdpSocket, msg: &T) -> Result<()> {
    let packet = encode_packet(PKT_UNRELIABLE, msg)?;
    socket.send(&packet).await?;
    Ok(())
}

async fn run_one_client(
    join: JoinMatchRes,
    trigger_param_update: bool,
    stats: Arc<Mutex<HashMap<String, ClientStats>>>,
    tick_ms: u64,
    ticks: u64,
    udp_override_url: Option<String>,
) -> Result<()> {
    let udp_url = udp_override_url.unwrap_or(join.udp_url.clone());
    let socket = connect_udp_socket(&udp_url).await?;

    send_reliable(
        &socket,
        &ClientReliable::Join {
            token: join.token.clone(),
        },
    )
    .await?;

    let mut buf = vec![0u8; 65536];
    loop {
        let len = socket.recv(&mut buf).await?;
        let packet = &buf[..len];
        match parse_reliable(packet) {
            Some(ServerReliable::JoinOk { player_id, params, .. }) => {
                if player_id != join.player_id {
                    return Err(anyhow!(
                        "joined with unexpected player_id: expected {} got {}",
                        join.player_id,
                        player_id
                    ));
                }
                let mut all = stats.lock().await;
                let st = all.entry(join.player_id.clone()).or_default();
                st.join_params = Some(params);
                break;
            }
            Some(ServerReliable::Error { code, message }) => {
                return Err(anyhow!("join failed: {code} {message}"));
            }
            Some(ServerReliable::MatchStarted { .. } | ServerReliable::ParamApplied { .. }) => {}
            None => {}
        }
    }

    let (reliable_tx, mut reliable_rx) = mpsc::channel::<ServerReliable>(1024);
    let socket_reader = socket.clone();
    let stat_map_for_reader = stats.clone();
    let pid_for_reader = join.player_id.clone();
    let reader_task = tokio::spawn(async move {
        let mut recv_buf = vec![0u8; 65536];
        loop {
            let len = match socket_reader.recv(&mut recv_buf).await {
                Ok(v) => v,
                Err(_) => break,
            };
            let packet = &recv_buf[..len];

            if let Some(msg) = parse_reliable(packet) {
                if reliable_tx.send(msg).await.is_err() {
                    break;
                }
                continue;
            }

            let ServerDatagram::Pos { player_id, .. } = match parse_datagram(packet) {
                Some(v) => v,
                None => continue,
            };
            let mut all = stat_map_for_reader.lock().await;
            let st = all.entry(pid_for_reader.clone()).or_default();
            st.datagram_count += 1;
            st.datagram_from_players.insert(player_id);
        }
    });

    if trigger_param_update {
        sleep(Duration::from_millis(400)).await;
        send_reliable(
            &socket,
            &ClientReliable::ParamChange {
                seq: 1,
                param: ParamKind::Gravity,
                direction: ParamDirection::Increase,
            },
        )
        .await?;

        sleep(Duration::from_millis(50)).await;
        send_reliable(
            &socket,
            &ClientReliable::ParamChange {
                seq: 2,
                param: ParamKind::Gravity,
                direction: ParamDirection::Increase,
            },
        )
        .await?;
    }

    for seq in 1..=ticks {
        let dg = ClientDatagram::Pos {
            seq,
            x: seq as f32 * 0.1,
            y: seq as f32 * 0.2,
            vx: 0.1,
            vy: -0.1,
        };
        send_datagram(&socket, &dg).await?;
        sleep(Duration::from_millis(tick_ms)).await;

        while let Ok(msg) = reliable_rx.try_recv() {
            match msg {
                ServerReliable::ParamApplied {
                    from_player_id,
                    params,
                    next_param_change_at_unix,
                    ..
                } => {
                    let mut all = stats.lock().await;
                    let st = all.entry(join.player_id.clone()).or_default();
                    st.param_applied_count += 1;
                    if from_player_id == join.player_id {
                        st.observed_param_state = Some(params);
                        st.next_param_change_at_unix = Some(next_param_change_at_unix);
                    }
                }
                ServerReliable::Error { code, message } => {
                    if trigger_param_update
                        && code == "param_update_failed"
                        && message.starts_with("cooldown_active:")
                    {
                        let mut all = stats.lock().await;
                        let st = all.entry(join.player_id.clone()).or_default();
                        st.cooldown_error_seen = true;
                    }
                }
                ServerReliable::JoinOk { .. } | ServerReliable::MatchStarted { .. } => {}
            }
        }
    }

    sleep(Duration::from_millis(300)).await;
    while let Ok(msg) = reliable_rx.try_recv() {
        match msg {
            ServerReliable::ParamApplied {
                from_player_id,
                params,
                next_param_change_at_unix,
                ..
            } => {
                let mut all = stats.lock().await;
                let st = all.entry(join.player_id.clone()).or_default();
                st.param_applied_count += 1;
                if from_player_id == join.player_id {
                    st.observed_param_state = Some(params);
                    st.next_param_change_at_unix = Some(next_param_change_at_unix);
                }
            }
            ServerReliable::Error { code, message } => {
                if trigger_param_update
                    && code == "param_update_failed"
                    && message.starts_with("cooldown_active:")
                {
                    let mut all = stats.lock().await;
                    let st = all.entry(join.player_id.clone()).or_default();
                    st.cooldown_error_seen = true;
                }
            }
            ServerReliable::JoinOk { .. } | ServerReliable::MatchStarted { .. } => {}
        }
    }

    sleep(Duration::from_millis(SNAPSHOT_LINGER_MS)).await;
    reader_task.abort();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let api_base = std::env::var("API_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let tick_ms = std::env::var("TICK_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TICK_MS);
    let ticks = std::env::var("TICKS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TICKS);
    let udp_override_url = std::env::var("UDP_OVERRIDE_URL")
        .ok()
        .or_else(|| std::env::var("QUIC_OVERRIDE_URL").ok());

    let http = reqwest::Client::new();

    let create: CreateMatchRes = http
        .post(format!("{api_base}/v1/matches"))
        .json(&serde_json::json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    println!(
        "created match: {} max={}",
        create.match_id, create.max_players
    );

    let mut joins = Vec::new();
    for i in 0..4 {
        let res: JoinMatchRes = http
            .post(format!("{api_base}/v1/matches/{}/join", create.match_id))
            .json(&serde_json::json!({"display_name": format!("p{}", i + 1)}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        joins.push(res);
    }

    let trigger_player_id = joins
        .first()
        .map(|join| join.player_id.clone())
        .ok_or_else(|| anyhow!("join list is empty"))?;

    let stats: Arc<Mutex<HashMap<String, ClientStats>>> = Arc::new(Mutex::new(HashMap::new()));

    let mut tasks = Vec::new();
    for (idx, join) in joins.into_iter().enumerate() {
        let st = stats.clone();
        tasks.push(tokio::spawn(run_one_client(
            join,
            idx == 0,
            st,
            tick_ms,
            ticks,
            udp_override_url.clone(),
        )));
    }

    sleep(Duration::from_millis(tick_ms * ticks + 1200)).await;

    let room: GetMatchRes = http
        .get(format!("{api_base}/v1/matches/{}", create.match_id))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    println!(
        "room snapshot: match_id={} max_players={} started_at={} players={}",
        room.match_id,
        room.max_players,
        room.started_at_unix,
        room.players.len()
    );
    for p in &room.players {
        println!(
            "{}({}): gravity={} friction={} speed={} next_change={}",
            p.player_id,
            p.display_name,
            p.gravity,
            p.friction,
            p.speed,
            p.next_param_change_at_unix
        );
    }

    for t in tasks {
        t.await??;
    }

    let s = stats.lock().await;
    println!("=== client stats ===");
    for (pid, st) in s.iter() {
        println!(
            "{}: param_applied={}, datagrams={}, from_players={}, cooldown_error_seen={}",
            pid,
            st.param_applied_count,
            st.datagram_count,
            st.datagram_from_players.len(),
            st.cooldown_error_seen
        );
    }

    if s.len() != 4 {
        return Err(anyhow!("expected 4 clients in stats, got {}", s.len()));
    }

    for (pid, st) in s.iter() {
        let join_params = st
            .join_params
            .as_ref()
            .ok_or_else(|| anyhow!("{pid} missing join params"))?;
        if join_params.gravity != 2 || join_params.friction != 2 || join_params.speed != 2 {
            return Err(anyhow!(
                "{pid} expected default params 2/2/2, got {}/{}/{}",
                join_params.gravity,
                join_params.friction,
                join_params.speed
            ));
        }
        if st.param_applied_count == 0 {
            return Err(anyhow!("{pid} did not receive param_applied"));
        }
        if st.datagram_from_players.len() < 2 {
            return Err(anyhow!(
                "{pid} received datagrams from too few players: {}",
                st.datagram_from_players.len()
            ));
        }
    }

    let trigger_stats = s
        .get(&trigger_player_id)
        .ok_or_else(|| anyhow!("missing trigger player stats"))?;
    if !trigger_stats.cooldown_error_seen {
        return Err(anyhow!("trigger player did not observe cooldown rejection"));
    }
    let observed = trigger_stats
        .observed_param_state
        .as_ref()
        .ok_or_else(|| anyhow!("trigger player missing observed param state"))?;
    if observed.gravity != 3 || observed.friction != 2 || observed.speed != 2 {
        return Err(anyhow!(
            "trigger player observed wrong param state: {}/{}/{}",
            observed.gravity,
            observed.friction,
            observed.speed
        ));
    }
    if trigger_stats.next_param_change_at_unix.unwrap_or_default() == 0 {
        return Err(anyhow!("trigger player missing cooldown timestamp"));
    }

    let trigger_room_player = room
        .players
        .iter()
        .find(|p| p.player_id == trigger_player_id)
        .ok_or_else(|| anyhow!("trigger player missing from room snapshot"))?;
    if trigger_room_player.gravity != 3
        || trigger_room_player.friction != 2
        || trigger_room_player.speed != 2
    {
        return Err(anyhow!(
            "room snapshot state mismatch: {}/{}/{}",
            trigger_room_player.gravity,
            trigger_room_player.friction,
            trigger_room_player.speed
        ));
    }
    if trigger_room_player.next_param_change_at_unix == 0 {
        return Err(anyhow!("room snapshot missing cooldown timestamp"));
    }

    println!("PASS: defaults, one-step param change, cooldown, internal state, and datagram sync verified");
    Ok(())
}
