use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::magicnums::ROOM_MAX_PLAYERS;
use crate::types::{AppState, PlayerConnection, PlayerHandle, PlayerParams, ServerReliable};
use crate::utils::now_unix;

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
    let next_param_change_at_unix = room
        .players
        .get(player_id)
        .map(|p| p.next_param_change_at_unix)
        .unwrap_or(0);
    let color_index = room.players.get(player_id).and_then(|p| p.color_index);
    let stage_order = room
        .players
        .get(player_id)
        .map(|p| p.stage_order.clone())
        .unwrap_or_default();
    let current_stage_index = room
        .players
        .get(player_id)
        .map(|p| p.current_stage_index)
        .unwrap_or(0);

    room.players.insert(
        player_id.to_string(),
        PlayerHandle {
            display_name,
            params: params.clone(),
            next_param_change_at_unix,
            color_index,
            stage_order,
            current_stage_index,
            reliable_tx,
            connection: Some(PlayerConnection::Udp(addr)),
        },
    );
    room.last_activity_unix = now_unix();

    Ok(params)
}

pub(crate) async fn remove_player(st: &AppState, match_id: &str, player_id: &str) {
    let mut core = st.core.lock().await;
    if let Some(room) = core.matches.get_mut(match_id) {
        room.players.remove(player_id);
        room.last_activity_unix = now_unix();
    }
}
