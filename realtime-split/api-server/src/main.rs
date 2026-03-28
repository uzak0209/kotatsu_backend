use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use kotatsu_proto::controlplane::v1::{
    control_plane_client::ControlPlaneClient, CreateRoomRequest, DeleteRoomRequest, GetRoomRequest,
    IssueJoinTicketRequest, ListRoomsRequest, StartRoomRequest,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::info;
use utoipa::{OpenApi, ToSchema};

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

#[derive(Debug, Serialize, ToSchema)]
struct MatchSummaryRes {
    match_id: String,
    max_players: u32,
    player_count: u32,
    started_at_unix: u64,
    players: Vec<RoomPlayerRes>,
}

#[derive(Debug, Serialize, ToSchema)]
struct ListMatchesRes {
    matches: Vec<MatchSummaryRes>,
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
    started_at_unix: u64,
    players: Vec<RoomPlayerRes>,
}

#[derive(Debug, Serialize, ToSchema)]
struct StartMatchRes {
    match_id: String,
    started_at_unix: u64,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        openapi,
        list_matches,
        create_match,
        get_match,
        delete_match,
        start_match,
        join_match
    ),
    components(
        schemas(
            HealthRes,
            ErrorRes,
            CreateMatchReq,
            CreateMatchRes,
            MatchSummaryRes,
            ListMatchesRes,
            JoinMatchReq,
            JoinMatchRes,
            RoomPlayerRes,
            GetMatchRes,
            StartMatchRes
        )
    ),
    tags(
        (name = "kotatsu-api", description = "Kotatsu split matchmaking API")
    )
)]
struct ApiDoc;

#[utoipa::path(
    get,
    path = "/health",
    tag = "kotatsu-api",
    responses(
        (status = 200, description = "Health status", body = HealthRes)
    )
)]
async fn health() -> impl IntoResponse {
    Json(HealthRes { ok: true })
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    tag = "kotatsu-api",
    responses(
        (status = 200, description = "Generated OpenAPI spec")
    )
)]
async fn openapi() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

#[utoipa::path(
    get,
    path = "/v1/matches",
    tag = "kotatsu-api",
    responses(
        (status = 200, description = "List all current matches", body = ListMatchesRes),
        (status = 502, description = "Control plane failure", body = ErrorRes)
    )
)]
async fn list_matches(State(st): State<AppState>) -> impl IntoResponse {
    let mut grpc = st.grpc.lock().await;
    match grpc.list_rooms(ListRoomsRequest {}).await {
        Ok(res) => {
            let r = res.into_inner();
            (
                StatusCode::OK,
                Json(ListMatchesRes {
                    matches: r
                        .rooms
                        .into_iter()
                        .map(|room| MatchSummaryRes {
                            match_id: room.match_id,
                            max_players: room.max_players,
                            player_count: room.players.len() as u32,
                            started_at_unix: room.started_at_unix,
                            players: room
                                .players
                                .into_iter()
                                .map(|p| RoomPlayerRes {
                                    player_id: p.player_id,
                                    display_name: p.display_name,
                                    gravity: p.gravity,
                                    friction: p.friction,
                                    speed: p.speed,
                                    next_param_change_at_unix: p.next_param_change_at_unix,
                                })
                                .collect(),
                        })
                        .collect(),
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorRes {
                error: format!("control_plane_error:{e}"),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/v1/matches",
    tag = "kotatsu-api",
    request_body = CreateMatchReq,
    responses(
        (status = 200, description = "Create a new match", body = CreateMatchRes),
        (status = 502, description = "Control plane failure", body = ErrorRes)
    )
)]
async fn create_match(
    State(st): State<AppState>,
    Json(_req): Json<CreateMatchReq>,
) -> impl IntoResponse {
    let mut grpc = st.grpc.lock().await;
    match grpc.create_room(CreateRoomRequest {}).await {
        Ok(res) => {
            let r = res.into_inner();
            (
                StatusCode::OK,
                Json(CreateMatchRes {
                    match_id: r.match_id,
                    max_players: r.max_players,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorRes {
                error: format!("control_plane_error:{e}"),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/v1/matches/{match_id}/join",
    tag = "kotatsu-api",
    params(
        ("match_id" = String, Path, description = "Match id")
    ),
    request_body = JoinMatchReq,
    responses(
        (status = 200, description = "Issue join ticket", body = JoinMatchRes),
        (status = 404, description = "Match not found", body = ErrorRes),
        (status = 409, description = "Match is full", body = ErrorRes),
        (status = 502, description = "Control plane failure", body = ErrorRes)
    )
)]
async fn join_match(
    State(st): State<AppState>,
    Path(match_id): Path<String>,
    Json(req): Json<JoinMatchReq>,
) -> impl IntoResponse {
    let mut grpc = st.grpc.lock().await;
    let request = IssueJoinTicketRequest {
        match_id,
        display_name: req.display_name.unwrap_or_default(),
    };

    match grpc.issue_join_ticket(request).await {
        Ok(res) => {
            let r = res.into_inner();
            (
                StatusCode::OK,
                Json(JoinMatchRes {
                    match_id: r.match_id,
                    player_id: r.player_id,
                    token: r.token,
                    udp_url: r.udp_url,
                    token_expires_at_unix: r.token_expires_at_unix,
                }),
            )
                .into_response()
        }
        Err(e) if e.code() == tonic::Code::NotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorRes {
                error: "match_not_found".into(),
            }),
        )
            .into_response(),
        Err(e) if e.code() == tonic::Code::FailedPrecondition => (
            StatusCode::CONFLICT,
            Json(ErrorRes {
                error: "match_full".into(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorRes {
                error: format!("control_plane_error:{e}"),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/v1/matches/{match_id}",
    tag = "kotatsu-api",
    params(
        ("match_id" = String, Path, description = "Match id")
    ),
    responses(
        (status = 200, description = "Get room snapshot", body = GetMatchRes),
        (status = 404, description = "Match not found", body = ErrorRes),
        (status = 502, description = "Control plane failure", body = ErrorRes)
    )
)]
async fn get_match(State(st): State<AppState>, Path(match_id): Path<String>) -> impl IntoResponse {
    let mut grpc = st.grpc.lock().await;
    match grpc.get_room(GetRoomRequest { match_id }).await {
        Ok(res) => {
            let r = res.into_inner();
            (
                StatusCode::OK,
                Json(GetMatchRes {
                    match_id: r.match_id,
                    max_players: r.max_players,
                    started_at_unix: r.started_at_unix,
                    players: r
                        .players
                        .into_iter()
                        .map(|p| RoomPlayerRes {
                            player_id: p.player_id,
                            display_name: p.display_name,
                            gravity: p.gravity,
                            friction: p.friction,
                            speed: p.speed,
                            next_param_change_at_unix: p.next_param_change_at_unix,
                        })
                        .collect(),
                }),
            )
                .into_response()
        }
        Err(e) if e.code() == tonic::Code::NotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorRes {
                error: "match_not_found".into(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorRes {
                error: format!("control_plane_error:{e}"),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/v1/matches/{match_id}/start",
    tag = "kotatsu-api",
    params(
        ("match_id" = String, Path, description = "Match id")
    ),
    responses(
        (status = 200, description = "Start a match", body = StartMatchRes),
        (status = 404, description = "Match not found", body = ErrorRes),
        (status = 502, description = "Control plane failure", body = ErrorRes)
    )
)]
async fn start_match(
    State(st): State<AppState>,
    Path(match_id): Path<String>,
) -> impl IntoResponse {
    let mut grpc = st.grpc.lock().await;
    match grpc.start_room(StartRoomRequest { match_id }).await {
        Ok(res) => {
            let r = res.into_inner();
            (
                StatusCode::OK,
                Json(StartMatchRes {
                    match_id: r.match_id,
                    started_at_unix: r.started_at_unix,
                }),
            )
                .into_response()
        }
        Err(e) if e.code() == tonic::Code::NotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorRes {
                error: "match_not_found".into(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorRes {
                error: format!("control_plane_error:{e}"),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/v1/matches/{match_id}",
    tag = "kotatsu-api",
    params(
        ("match_id" = String, Path, description = "Match id")
    ),
    responses(
        (status = 204, description = "Delete a match"),
        (status = 404, description = "Match not found", body = ErrorRes),
        (status = 502, description = "Control plane failure", body = ErrorRes)
    )
)]
async fn delete_match(
    State(st): State<AppState>,
    Path(match_id): Path<String>,
) -> impl IntoResponse {
    let mut grpc = st.grpc.lock().await;
    match grpc.delete_room(DeleteRoomRequest { match_id }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) if e.code() == tonic::Code::NotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorRes {
                error: "match_not_found".into(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorRes {
                error: format!("control_plane_error:{e}"),
            }),
        )
            .into_response(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let api_addr: SocketAddr = std::env::var("API_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    let control_plane_url =
        std::env::var("CONTROL_PLANE_URL").unwrap_or_else(|_| "http://127.0.0.1:50051".into());

    let grpc = ControlPlaneClient::connect(control_plane_url).await?;

    let st = AppState {
        grpc: Arc::new(Mutex::new(grpc)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi))
        .route("/v1/matches", get(list_matches).post(create_match))
        .route("/v1/matches/:match_id", get(get_match).delete(delete_match))
        .route("/v1/matches/:match_id/start", post(start_match))
        .route("/v1/matches/:match_id/join", post(join_match))
        .with_state(st);

    let listener = tokio::net::TcpListener::bind(api_addr).await?;
    info!("api server listening on {api_addr}");
    axum::serve(listener, app).await?;

    Ok(())
}
