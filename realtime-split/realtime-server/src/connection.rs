use quinn::Connection;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::player::{register_player, remove_player};
use crate::protocol::{read_json_line, write_json_line};
use crate::room::{broadcast_datagram, broadcast_reliable, update_player_params};
use crate::ticket::consume_ticket;
use crate::types::{AppState, ClientDatagram, ClientReliable, ServerDatagram, ServerReliable};
use crate::utils::now_ms;

pub(crate) async fn handle_quic_connection(st: AppState, conn: Connection) {
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
    let params =
        match register_player(&st, &match_id, &player_id, display_name, &conn, tx.clone()).await {
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
            let updated =
                match update_player_params(&st, &match_id, &player_id, param, direction).await {
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
