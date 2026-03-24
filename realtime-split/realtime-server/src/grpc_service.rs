use kotatsu_proto::controlplane::v1::{
    control_plane_server::ControlPlane, CreateRoomRequest, CreateRoomResponse, GetRoomRequest,
    GetRoomResponse, IssueJoinTicketRequest, IssueJoinTicketResponse, RoomPlayer,
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::magicnums::{ROOM_MAX_PLAYERS, TOKEN_TTL_SECS};
use crate::types::{AppState, MatchRoom, Ticket};
use crate::utils::now_unix;

#[derive(Clone)]
pub(crate) struct ControlPlaneSvc {
    pub(crate) st: AppState,
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

        let players = room
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
            .collect();

        Ok(Response::new(GetRoomResponse {
            match_id: req.match_id,
            max_players: ROOM_MAX_PLAYERS as u32,
            players,
        }))
    }
}
