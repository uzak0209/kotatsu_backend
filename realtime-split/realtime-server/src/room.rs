use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::params::apply_param_change;
use crate::types::{
    AppState, ParamDirection, ParamKind, ParamMutation, PlayerConnection, ServerReliable,
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
