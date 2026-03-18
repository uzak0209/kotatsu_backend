use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use kotatsu_proto::controlplane::v1::{
    control_plane_client::ControlPlaneClient, CreateRoomRequest, GetRoomRequest, IssueJoinTicketRequest,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::info;

#[derive(Clone)]
struct AppState {
    grpc: Arc<Mutex<ControlPlaneClient<Channel>>>,
}

#[derive(Debug, Serialize)]
struct HealthRes {
    ok: bool,
}

#[derive(Debug, Serialize)]
struct ErrorRes {
    error: String,
}

#[derive(Debug, Deserialize)]
struct CreateMatchReq {}

#[derive(Debug, Serialize)]
struct CreateMatchRes {
    match_id: String,
    max_players: u32,
}

#[derive(Debug, Deserialize)]
struct JoinMatchReq {
    display_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct JoinMatchRes {
    match_id: String,
    player_id: String,
    token: String,
    quic_url: String,
    token_expires_at_unix: u64,
}

#[derive(Debug, Serialize)]
struct RoomPlayerRes {
    player_id: String,
    display_name: String,
    gravity: u32,
    friction: u32,
    speed: u32,
    next_param_change_at_unix: u64,
}

#[derive(Debug, Serialize)]
struct GetMatchRes {
    match_id: String,
    max_players: u32,
    players: Vec<RoomPlayerRes>,
}

async fn health() -> impl IntoResponse {
    Json(HealthRes { ok: true })
}

async fn create_match(State(st): State<AppState>, Json(_): Json<CreateMatchReq>) -> impl IntoResponse {
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
                    quic_url: r.quic_url,
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
        .route("/v1/matches", post(create_match))
        .route("/v1/matches/:match_id", get(get_match))
        .route("/v1/matches/:match_id/join", post(join_match))
        .with_state(st);

    let listener = tokio::net::TcpListener::bind(api_addr).await?;
    info!("api server listening on {api_addr}");
    axum::serve(listener, app).await?;

    Ok(())
}
