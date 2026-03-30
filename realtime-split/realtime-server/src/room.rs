use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::magicnums::{MATCH_STAGE_ORDER_LEN, MATCH_STAGE_POOL_SIZE, ROOM_MAX_PLAYERS};
use crate::params::apply_param_change;
use crate::types::{
    AppState, MatchRoom, ParamDirection, ParamKind, ParamMutation, PlayerConnection,
    PlayerMatchState, ServerReliable,
};
use crate::utils::now_unix;

pub(crate) async fn update_player_params(
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
    room.last_activity_unix = now_unix();
    Ok(updated)
}

pub(crate) async fn delete_stale_lobby_rooms(st: &AppState, idle_timeout_secs: u64) -> Vec<String> {
    let now = now_unix();
    let mut core = st.core.lock().await;
    let stale_match_ids = core
        .matches
        .iter()
        .filter(|(_, room)| {
            room.started_at_unix == 0
                && now.saturating_sub(room.last_activity_unix) >= idle_timeout_secs
        })
        .map(|(match_id, _)| match_id.clone())
        .collect::<Vec<_>>();

    if stale_match_ids.is_empty() {
        return stale_match_ids;
    }

    for match_id in &stale_match_ids {
        core.matches.remove(match_id);
    }
    core.tickets
        .retain(|_, ticket| !stale_match_ids.contains(&ticket.match_id));

    stale_match_ids
}

pub(crate) async fn broadcast_reliable(st: &AppState, match_id: &str, msg: ServerReliable) {
    let senders: Vec<mpsc::Sender<ServerReliable>> = {
        let core = st.core.lock().await;
        let Some(room) = core.matches.get(match_id) else {
            return;
        };
        room.players
            .values()
            .map(|p| p.reliable_tx.clone())
            .collect()
    };

    for tx in senders {
        let _ = tx.send(msg.clone()).await;
    }
}

pub(crate) async fn broadcast_reliable_udp(st: &AppState, match_id: &str, msg: ServerReliable) {
    let senders: Vec<mpsc::Sender<ServerReliable>> = {
        let core = st.core.lock().await;
        let Some(room) = core.matches.get(match_id) else {
            return;
        };
        room.players
            .values()
            .map(|p| p.reliable_tx.clone())
            .collect()
    };

    for tx in senders {
        let _ = tx.send(msg.clone()).await;
    }
}

pub(crate) async fn broadcast_datagram_udp(
    st: &AppState,
    match_id: &str,
    sender_player_id: &str,
    payload: Vec<u8>,
) {
    let addrs: Vec<SocketAddr> = {
        let core = st.core.lock().await;
        let Some(room) = core.matches.get(match_id) else {
            return;
        };
        room.players
            .iter()
            .filter_map(|(pid, p)| {
                if pid == sender_player_id {
                    return None;
                }
                match &p.connection {
                    Some(PlayerConnection::Udp(addr)) => Some(*addr),
                    None => None,
                }
            })
            .collect()
    };

    // Send unreliable datagram to all addresses
    if let Some(socket) = &st.udp_socket {
        const PKT_UNRELIABLE: u8 = 0x02;
        let mut packet = vec![PKT_UNRELIABLE];
        packet.extend_from_slice(&payload);

        for addr in addrs {
            let _ = socket.send_to(&packet, addr).await;
        }
    }
}

pub(crate) async fn broadcast_datagram_udp_to_all(st: &AppState, match_id: &str, payload: Vec<u8>) {
    let addrs: Vec<SocketAddr> = {
        let core = st.core.lock().await;
        let Some(room) = core.matches.get(match_id) else {
            return;
        };
        room.players
            .values()
            .filter_map(|p| match &p.connection {
                Some(PlayerConnection::Udp(addr)) => Some(*addr),
                None => None,
            })
            .collect()
    };

    if let Some(socket) = &st.udp_socket {
        const PKT_UNRELIABLE: u8 = 0x02;
        let mut packet = vec![PKT_UNRELIABLE];
        packet.extend_from_slice(&payload);

        for addr in addrs {
            let _ = socket.send_to(&packet, addr).await;
        }
    }
}

pub(crate) async fn start_match_and_snapshot_players(
    st: &AppState,
    match_id: &str,
) -> Result<(u64, bool, Vec<PlayerMatchState>)> {
    let mut core = st.core.lock().await;
    let room = core
        .matches
        .get_mut(match_id)
        .ok_or_else(|| anyhow!("match_not_found"))?;

    let just_started = room.started_at_unix == 0;
    if just_started {
        room.started_at_unix = now_unix();
        room.last_activity_unix = room.started_at_unix;
        assign_randomized_layout(room);
    }

    let started_at_unix = room.started_at_unix;
    let players = snapshot_players(room);
    Ok((started_at_unix, just_started, players))
}

pub(crate) async fn update_player_stage_progress(
    st: &AppState,
    match_id: &str,
    player_id: &str,
    current_stage_index: u8,
) -> Result<u8> {
    let mut core = st.core.lock().await;
    let room = core
        .matches
        .get_mut(match_id)
        .ok_or_else(|| anyhow!("match_not_found"))?;
    let player = room
        .players
        .get_mut(player_id)
        .ok_or_else(|| anyhow!("player_not_found"))?;

    let max_stage_index = player.stage_order.len().saturating_add(1) as u8;
    let clamped = current_stage_index.min(max_stage_index);
    player.current_stage_index = clamped;
    room.last_activity_unix = now_unix();
    Ok(clamped)
}

fn assign_randomized_layout(room: &mut MatchRoom) {
    let mut rng = rand::thread_rng();
    let mut player_ids = room.players.keys().cloned().collect::<Vec<_>>();
    player_ids.sort();

    let mut color_indices = (0..ROOM_MAX_PLAYERS as u8).collect::<Vec<_>>();
    color_indices.shuffle(&mut rng);

    for (index, player_id) in player_ids.iter().enumerate() {
        let Some(player) = room.players.get_mut(player_id) else {
            continue;
        };

        let mut stage_order = (0..MATCH_STAGE_POOL_SIZE as u8).collect::<Vec<_>>();
        stage_order.shuffle(&mut rng);
        stage_order.truncate(MATCH_STAGE_ORDER_LEN.min(stage_order.len()));

        player.color_index = color_indices.get(index).copied();
        player.stage_order = stage_order;
        player.current_stage_index = 0;
    }
}

fn snapshot_players(room: &MatchRoom) -> Vec<PlayerMatchState> {
    let mut players = room
        .players
        .iter()
        .map(|(player_id, player)| PlayerMatchState {
            player_id: player_id.clone(),
            display_name: player.display_name.clone(),
            color_index: player.color_index.unwrap_or(0),
            stage_order: player.stage_order.clone(),
            current_stage_index: player.current_stage_index,
        })
        .collect::<Vec<_>>();
    players.sort_by(|a, b| a.player_id.cmp(&b.player_id));
    players
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CoreState, MatchRoom, PlayerHandle, PlayerParams};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

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
                                color_index: None,
                                stage_order: Vec::new(),
                                current_stage_index: 0,
                                reliable_tx: mpsc::channel(1).0,
                                connection: None,
                            },
                        )]),
                        started_at_unix: 0,
                        last_activity_unix: 0,
                    },
                )]),
                tickets: HashMap::new(),
            })),
            udp_public_url: "udp://127.0.0.1:4433".into(),
            udp_socket: None,
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
                                color_index: None,
                                stage_order: Vec::new(),
                                current_stage_index: 0,
                                reliable_tx: mpsc::channel(1).0,
                                connection: None,
                            },
                        )]),
                        started_at_unix: 0,
                        last_activity_unix: 0,
                    },
                )]),
                tickets: HashMap::new(),
            })),
            udp_public_url: "udp://127.0.0.1:4433".into(),
            udp_socket: None,
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

    #[tokio::test]
    async fn delete_stale_lobby_rooms_removes_only_unstarted_expired_rooms() {
        let stale_match_id = "m_stale".to_string();
        let active_match_id = "m_active".to_string();
        let started_match_id = "m_started".to_string();
        let now = now_unix();
        let st = AppState {
            core: Arc::new(Mutex::new(CoreState {
                matches: HashMap::from([
                    (
                        stale_match_id.clone(),
                        MatchRoom {
                            players: HashMap::new(),
                            started_at_unix: 0,
                            last_activity_unix: now.saturating_sub(21),
                        },
                    ),
                    (
                        active_match_id.clone(),
                        MatchRoom {
                            players: HashMap::new(),
                            started_at_unix: 0,
                            last_activity_unix: now,
                        },
                    ),
                    (
                        started_match_id.clone(),
                        MatchRoom {
                            players: HashMap::new(),
                            started_at_unix: now.saturating_sub(100),
                            last_activity_unix: now.saturating_sub(100),
                        },
                    ),
                ]),
                tickets: HashMap::from([
                    (
                        "t_stale".into(),
                        crate::types::Ticket {
                            match_id: stale_match_id.clone(),
                            player_id: "p_stale".into(),
                            display_name: "stale".into(),
                            expires_at_unix: now + 100,
                        },
                    ),
                    (
                        "t_active".into(),
                        crate::types::Ticket {
                            match_id: active_match_id.clone(),
                            player_id: "p_active".into(),
                            display_name: "active".into(),
                            expires_at_unix: now + 100,
                        },
                    ),
                ]),
            })),
            udp_public_url: "udp://127.0.0.1:4433".into(),
            udp_socket: None,
        };

        let deleted = delete_stale_lobby_rooms(&st, 20).await;
        assert_eq!(deleted, vec![stale_match_id.clone()]);

        let core = st.core.lock().await;
        assert!(!core.matches.contains_key(&stale_match_id));
        assert!(core.matches.contains_key(&active_match_id));
        assert!(core.matches.contains_key(&started_match_id));
        assert!(!core.tickets.contains_key("t_stale"));
        assert!(core.tickets.contains_key("t_active"));
    }
}
