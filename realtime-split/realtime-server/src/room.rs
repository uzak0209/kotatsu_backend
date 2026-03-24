use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::params::apply_param_change;
use crate::types::{AppState, ParamDirection, ParamKind, ParamMutation, PlayerConnection, ServerReliable};
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
    Ok(updated)
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
}
