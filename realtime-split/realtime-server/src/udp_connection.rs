use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

use crate::player::{register_player_udp, remove_player};
use crate::room::{broadcast_datagram_udp, broadcast_reliable_udp, update_player_params};
use crate::ticket::consume_ticket;
use crate::types::{AppState, ClientDatagram, ClientReliable, ServerReliable};
use crate::utils::now_ms;

// Packet type identifiers
const PKT_RELIABLE: u8 = 0x01;
const PKT_UNRELIABLE: u8 = 0x02;

/// Client session state
struct ClientSession {
    match_id: String,
    player_id: String,
    addr: SocketAddr,
    tx: mpsc::Sender<ServerReliable>,
    rx: Arc<Mutex<mpsc::Receiver<ServerReliable>>>,
}

type ClientRegistry = Arc<Mutex<HashMap<SocketAddr, ClientSession>>>;

pub async fn run_udp_server(mut st: AppState, bind_addr: SocketAddr) -> anyhow::Result<()> {
    let socket = Arc::new(UdpSocket::bind(bind_addr).await?);
    info!("UDP realtime server listening on {bind_addr}");

    // Store socket in AppState for broadcast_datagram_udp
    st.udp_socket = Some(socket.clone());

    let clients: ClientRegistry = Arc::new(Mutex::new(HashMap::new()));

    // Spawn task to handle outgoing reliable messages
    let socket_send = socket.clone();
    let clients_send = clients.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            let clients_guard = clients_send.lock().await;

            for session in clients_guard.values() {
                // Try to receive messages without blocking
                let mut rx_guard = session.rx.lock().await;
                while let Ok(msg) = rx_guard.try_recv() {
                    if let Ok(json) = serde_json::to_vec(&msg) {
                        let mut packet = vec![PKT_RELIABLE];
                        packet.extend_from_slice(&json);
                        let _ = socket_send.send_to(&packet, session.addr).await;
                    }
                }
            }
        }
    });

    let mut buf = vec![0u8; 65536];

    loop {
        let (len, addr) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                warn!("udp recv error: {e}");
                continue;
            }
        };

        if len == 0 {
            continue;
        }

        let packet = buf[..len].to_vec(); // Clone the data before spawning
        let st_clone = st.clone();
        let socket_clone = socket.clone();
        let clients_clone = clients.clone();

        tokio::spawn(async move {
            handle_udp_packet(st_clone, socket_clone, clients_clone, addr, packet).await;
        });
    }
}

async fn handle_udp_packet(
    st: AppState,
    socket: Arc<UdpSocket>,
    clients: ClientRegistry,
    addr: SocketAddr,
    packet: Vec<u8>,
) {
    if packet.is_empty() {
        return;
    }

    let pkt_type = packet[0];
    let payload = &packet[1..];

    match pkt_type {
        PKT_RELIABLE => {
            handle_reliable_message(st, socket, clients, addr, payload).await;
        }
        PKT_UNRELIABLE => {
            handle_unreliable_message(st, socket, clients, addr, payload).await;
        }
        _ => {
            warn!("unknown packet type: {pkt_type:#x} from {addr}");
        }
    }
}

async fn handle_reliable_message(
    st: AppState,
    socket: Arc<UdpSocket>,
    clients: ClientRegistry,
    addr: SocketAddr,
    payload: &[u8],
) {
    let msg: ClientReliable = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(e) => {
            warn!("failed to parse reliable message from {addr}: {e}");
            return;
        }
    };

    match msg {
        ClientReliable::Join { token } => {
            handle_join(st, socket, clients, addr, token).await;
        }
        ClientReliable::ParamChange {
            seq,
            param,
            direction,
        } => {
            handle_param_change(st, clients, addr, seq, param, direction).await;
        }
    }
}

async fn handle_join(
    st: AppState,
    socket: Arc<UdpSocket>,
    clients: ClientRegistry,
    addr: SocketAddr,
    token: String,
) {
    // Check if already connected
    {
        let clients_guard = clients.lock().await;
        if clients_guard.contains_key(&addr) {
            warn!("client {addr} already joined");
            return;
        }
    }

    // Consume ticket
    let (match_id, player_id, display_name) = match consume_ticket(&st, &token).await {
        Ok(v) => v,
        Err(e) => {
            send_error(&socket, addr, "auth_failed", &e.to_string()).await;
            return;
        }
    };

    // Create message channel
    let (tx, rx) = mpsc::channel::<ServerReliable>(1024);

    // Register player
    let params =
        match register_player_udp(&st, &match_id, &player_id, display_name, addr, tx.clone()).await
        {
            Ok(p) => p,
            Err(e) => {
                send_error(&socket, addr, "join_failed", &e.to_string()).await;
                return;
            }
        };

    // Add to client registry
    {
        let mut clients_guard = clients.lock().await;
        clients_guard.insert(
            addr,
            ClientSession {
                match_id: match_id.clone(),
                player_id: player_id.clone(),
                addr,
                tx: tx.clone(),
                rx: Arc::new(Mutex::new(rx)),
            },
        );
    }

    // Send JoinOk
    let join_ok = ServerReliable::JoinOk {
        match_id: match_id.clone(),
        player_id: player_id.clone(),
        params,
        server_time_ms: now_ms(),
    };

    if send_reliable(&socket, addr, &join_ok).await.is_err() {
        remove_client(&st, &clients, addr).await;
        return;
    }

    info!("player joined: match={match_id} player={player_id} addr={addr}");
}

async fn handle_param_change(
    st: AppState,
    clients: ClientRegistry,
    addr: SocketAddr,
    seq: u64,
    param: crate::types::ParamKind,
    direction: crate::types::ParamDirection,
) {
    let (match_id, player_id, tx) = {
        let clients_guard = clients.lock().await;
        match clients_guard.get(&addr) {
            Some(session) => (
                session.match_id.clone(),
                session.player_id.clone(),
                session.tx.clone(),
            ),
            None => {
                warn!("param_change from non-joined client: {addr}");
                return;
            }
        }
    };

    let updated = match update_player_params(&st, &match_id, &player_id, param, direction).await {
        Ok(v) => v,
        Err(e) => {
            let _ = tx
                .send(ServerReliable::Error {
                    code: "param_update_failed".into(),
                    message: e.to_string(),
                })
                .await;
            return;
        }
    };

    broadcast_reliable_udp(
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

async fn handle_unreliable_message(
    st: AppState,
    _socket: Arc<UdpSocket>,
    clients: ClientRegistry,
    addr: SocketAddr,
    payload: &[u8],
) {
    let msg: ClientDatagram = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(_) => return, // Silently ignore malformed datagrams
    };

    let ClientDatagram::Pos { seq, x, y, vx, vy } = msg;

    let (match_id, player_id) = {
        let clients_guard = clients.lock().await;
        match clients_guard.get(&addr) {
            Some(session) => (session.match_id.clone(), session.player_id.clone()),
            None => return, // Not joined yet
        }
    };

    let out = crate::types::ServerDatagram::Pos {
        player_id: player_id.clone(),
        seq,
        x,
        y,
        vx,
        vy,
        server_time_ms: now_ms(),
    };

    if let Ok(payload) = serde_json::to_vec(&out) {
        broadcast_datagram_udp(&st, &match_id, &player_id, payload).await;
    }
}

async fn send_reliable(
    socket: &UdpSocket,
    addr: SocketAddr,
    msg: &ServerReliable,
) -> anyhow::Result<()> {
    let json = serde_json::to_vec(msg)?;
    let mut packet = vec![PKT_RELIABLE];
    packet.extend_from_slice(&json);
    socket.send_to(&packet, addr).await?;
    Ok(())
}

async fn send_error(socket: &UdpSocket, addr: SocketAddr, code: &str, message: &str) {
    let err = ServerReliable::Error {
        code: code.into(),
        message: message.into(),
    };
    let _ = send_reliable(socket, addr, &err).await;
}

async fn remove_client(st: &AppState, clients: &ClientRegistry, addr: SocketAddr) {
    let session = {
        let mut clients_guard = clients.lock().await;
        clients_guard.remove(&addr)
    };

    if let Some(session) = session {
        remove_player(st, &session.match_id, &session.player_id).await;
        info!(
            "player disconnected: match={} player={} addr={}",
            session.match_id, session.player_id, addr
        );
    }
}
