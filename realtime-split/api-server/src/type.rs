#[derive(Clone)]
struct AppState {
    grpc: Arc<Mutex<ControlPlaneClient<Channel>>>,
}

#[derive(Debug, Serialize, ToSchema)]
struct HealthRes {
    ok: bool,
}

#[derive(Debug, Serialize, ToSchema)]
struct ErrorRes {
    error: String,
}

#[derive(Debug, Deserialize, ToSchema)]
struct CreateMatchReq {}

#[derive(Debug, Serialize, ToSchema)]
struct CreateMatchRes {
    match_id: String,
    max_players: u32,
}

#[derive(Debug, Deserialize, ToSchema)]
struct JoinMatchReq {
    display_name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
struct JoinMatchRes {
    match_id: String,
    player_id: String,
    token: String,
    udp_url: String,
    token_expires_at_unix: u64,
}

#[derive(Debug, Serialize, ToSchema)]
struct RoomPlayerRes {
    player_id: String,
    display_name: String,
    gravity: u32,
    friction: u32,
    speed: u32,
    next_param_change_at_unix: u64,
}

#[derive(Debug, Serialize, ToSchema)]
struct GetMatchRes {
    match_id: String,
    max_players: u32,
    players: Vec<RoomPlayerRes>,
}
