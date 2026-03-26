use kotatsu_proto::controlplane::v1::{
    control_plane_server::ControlPlane, CreateRoomRequest, CreateRoomResponse, DeleteRoomRequest,
    DeleteRoomResponse, GetRoomRequest, GetRoomResponse, IssueJoinTicketRequest,
    IssueJoinTicketResponse, ListRoomsRequest, ListRoomsResponse, RoomPlayer, RoomSummary,
    StartRoomRequest, StartRoomResponse,
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::magicnums::{ROOM_MAX_PLAYERS, TOKEN_TTL_SECS};
use crate::room::broadcast_reliable_udp;
use crate::types::{AppState, MatchRoom, ServerReliable, Ticket};
use crate::utils::{now_ms, now_unix};

#[derive(Clone)]
pub(crate) struct ControlPlaneSvc {
    pub(crate) st: AppState,
}

fn room_players(room: &MatchRoom) -> Vec<RoomPlayer> {
    let mut players = room
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
        .collect::<Vec<_>>();
    players.sort_by(|a, b| a.player_id.cmp(&b.player_id));
    players
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

    async fn list_rooms(
        &self,
        _request: Request<ListRoomsRequest>,
    ) -> Result<Response<ListRoomsResponse>, Status> {
        let core = self.st.core.lock().await;
        let mut rooms = core
            .matches
            .iter()
            .map(|(match_id, room)| RoomSummary {
                match_id: match_id.clone(),
                max_players: ROOM_MAX_PLAYERS as u32,
                players: room_players(room),
                started_at_unix: room.started_at_unix,
            })
            .collect::<Vec<_>>();
        rooms.sort_by(|a, b| a.match_id.cmp(&b.match_id));

        Ok(Response::new(ListRoomsResponse { rooms }))
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
            quic_url: self.st.udp_public_url.clone(), // Using udp_url instead of quic_url
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

        Ok(Response::new(GetRoomResponse {
            match_id: req.match_id,
            max_players: ROOM_MAX_PLAYERS as u32,
            players: room_players(room),
            started_at_unix: room.started_at_unix,
        }))
    }

    async fn start_room(
        &self,
        request: Request<StartRoomRequest>,
    ) -> Result<Response<StartRoomResponse>, Status> {
        let req = request.into_inner();
        let (started_at_unix, just_started) = {
            let mut core = self.st.core.lock().await;
            let room = core
                .matches
                .get_mut(&req.match_id)
                .ok_or_else(|| Status::not_found("match_not_found"))?;

            if room.started_at_unix == 0 {
                room.started_at_unix = now_unix();
                (room.started_at_unix, true)
            } else {
                (room.started_at_unix, false)
            }
        };

        if just_started {
            broadcast_reliable_udp(
                &self.st,
                &req.match_id,
                ServerReliable::MatchStarted {
                    match_id: req.match_id.clone(),
                    started_at_unix,
                    server_time_ms: now_ms(),
                },
            )
            .await;
        }

        Ok(Response::new(StartRoomResponse {
            match_id: req.match_id,
            started_at_unix,
        }))
    }

    async fn delete_room(
        &self,
        request: Request<DeleteRoomRequest>,
    ) -> Result<Response<DeleteRoomResponse>, Status> {
        let req = request.into_inner();
        let mut core = self.st.core.lock().await;

        if core.matches.remove(&req.match_id).is_none() {
            return Err(Status::not_found("match_not_found"));
        }

        core.tickets
            .retain(|_, ticket| ticket.match_id != req.match_id);

        Ok(Response::new(DeleteRoomResponse {}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use crate::types::{CoreState, PlayerHandle, PlayerParams, Ticket};

    fn test_state(core: CoreState) -> AppState {
        AppState {
            core: Arc::new(Mutex::new(core)),
            udp_public_url: "udp://127.0.0.1:4433".into(),
            udp_socket: None,
        }
    }

    fn test_player(display_name: &str) -> PlayerHandle {
        PlayerHandle {
            display_name: display_name.into(),
            params: PlayerParams::default(),
            next_param_change_at_unix: 0,
            reliable_tx: tokio::sync::mpsc::channel(1).0,
            connection: None,
        }
    }

    #[tokio::test]
    async fn list_rooms_returns_all_rooms_sorted() {
        let svc = ControlPlaneSvc {
            st: test_state(CoreState {
                matches: HashMap::from([
                    (
                        "m_b".into(),
                        MatchRoom {
                            players: HashMap::from([("p_b".into(), test_player("beta"))]),
                            started_at_unix: 200,
                        },
                    ),
                    (
                        "m_a".into(),
                        MatchRoom {
                            players: HashMap::from([
                                ("p_z".into(), test_player("zeta")),
                                ("p_a".into(), test_player("alpha")),
                            ]),
                            started_at_unix: 100,
                        },
                    ),
                ]),
                tickets: HashMap::new(),
            }),
        };

        let response = svc
            .list_rooms(Request::new(ListRoomsRequest {}))
            .await
            .expect("list should succeed")
            .into_inner();

        assert_eq!(response.rooms.len(), 2);
        assert_eq!(response.rooms[0].match_id, "m_a");
        assert_eq!(response.rooms[1].match_id, "m_b");
        assert_eq!(response.rooms[0].started_at_unix, 100);
        assert_eq!(response.rooms[1].started_at_unix, 200);
        assert_eq!(response.rooms[0].players.len(), 2);
        assert_eq!(response.rooms[0].players[0].player_id, "p_a");
        assert_eq!(response.rooms[0].players[1].player_id, "p_z");
        assert_eq!(response.rooms[1].players[0].player_id, "p_b");
    }

    #[tokio::test]
    async fn start_room_marks_room_started_and_notifies_players() {
        let match_id = "m_start".to_string();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let svc = ControlPlaneSvc {
            st: test_state(CoreState {
                matches: HashMap::from([(
                    match_id.clone(),
                    MatchRoom {
                        players: HashMap::from([(
                            "p_1".into(),
                            PlayerHandle {
                                display_name: "host".into(),
                                params: PlayerParams::default(),
                                next_param_change_at_unix: 0,
                                reliable_tx: tx,
                                connection: None,
                            },
                        )]),
                        started_at_unix: 0,
                    },
                )]),
                tickets: HashMap::new(),
            }),
        };

        let response = svc
            .start_room(Request::new(StartRoomRequest {
                match_id: match_id.clone(),
            }))
            .await
            .expect("start should succeed")
            .into_inner();

        assert_eq!(response.match_id, match_id);
        assert!(response.started_at_unix > 0);

        let core = svc.st.core.lock().await;
        let room = core.matches.get(&match_id).expect("room should exist");
        assert_eq!(room.started_at_unix, response.started_at_unix);
        drop(core);

        let msg = rx.recv().await.expect("match start should be broadcast");
        match msg {
            ServerReliable::MatchStarted {
                match_id: got_match_id,
                started_at_unix,
                ..
            } => {
                assert_eq!(got_match_id, match_id);
                assert_eq!(started_at_unix, response.started_at_unix);
            }
            other => panic!("unexpected reliable message: {other:?}"),
        }
    }

    #[tokio::test]
    async fn delete_room_removes_room_and_pending_tickets() {
        let match_id = "m_delete".to_string();
        let keep_match_id = "m_keep".to_string();
        let svc = ControlPlaneSvc {
            st: test_state(CoreState {
                matches: HashMap::from([
                    (match_id.clone(), MatchRoom::new()),
                    (keep_match_id.clone(), MatchRoom::new()),
                ]),
                tickets: HashMap::from([
                    (
                        "t_delete".into(),
                        Ticket {
                            match_id: match_id.clone(),
                            player_id: "p_delete".into(),
                            display_name: "delete".into(),
                            expires_at_unix: 100,
                        },
                    ),
                    (
                        "t_keep".into(),
                        Ticket {
                            match_id: keep_match_id.clone(),
                            player_id: "p_keep".into(),
                            display_name: "keep".into(),
                            expires_at_unix: 100,
                        },
                    ),
                ]),
            }),
        };

        svc.delete_room(Request::new(DeleteRoomRequest {
            match_id: match_id.clone(),
        }))
        .await
        .expect("delete should succeed");

        let core = svc.st.core.lock().await;
        assert!(!core.matches.contains_key(&match_id));
        assert!(core.matches.contains_key(&keep_match_id));
        assert!(!core.tickets.contains_key("t_delete"));
        assert!(core.tickets.contains_key("t_keep"));
    }

    #[tokio::test]
    async fn delete_room_returns_not_found_for_missing_match() {
        let svc = ControlPlaneSvc {
            st: test_state(CoreState::default()),
        };

        let err = svc
            .delete_room(Request::new(DeleteRoomRequest {
                match_id: "missing".into(),
            }))
            .await
            .expect_err("missing match should fail");

        assert_eq!(err.code(), tonic::Code::NotFound);
        assert_eq!(err.message(), "match_not_found");
    }
}
