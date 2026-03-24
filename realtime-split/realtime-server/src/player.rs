use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::magicnums::ROOM_MAX_PLAYERS;
use crate::types::{AppState, PlayerConnection, PlayerHandle, PlayerParams, ServerReliable};

pub(crate) async fn register_player_udp(
    st: &AppState,
    match_id: &str,
    player_id: &str,
    display_name: String,
    addr: SocketAddr,
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
            connection: Some(PlayerConnection::Udp(addr)),
        },
    );

    Ok(params)
}

pub(crate) async fn remove_player(st: &AppState, match_id: &str, player_id: &str) {
    let mut core = st.core.lock().await;
    if let Some(room) = core.matches.get_mut(match_id) {
        room.players.remove(player_id);
    }
}
