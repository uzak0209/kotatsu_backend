use anyhow::Result;
use kotatsu_proto::controlplane::v1::control_plane_server::ControlPlaneServer;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tracing::info;

mod grpc_service;
mod magicnums;
mod params;
mod player;
mod room;
mod ticket;
mod types;
mod udp_connection;
mod utils;

use grpc_service::ControlPlaneSvc;
use types::{AppState, CoreState};
use udp_connection::run_udp_server;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let grpc_addr: SocketAddr = std::env::var("GRPC_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".into())
        .parse()?;
    let udp_bind_addr: SocketAddr = std::env::var("UDP_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:4433".into())
        .parse()?;
    let udp_public_url = std::env::var("UDP_PUBLIC_URL").unwrap_or_else(|_| {
        let host = std::env::var("PUBLIC_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("UDP_PORT").unwrap_or_else(|_| "4433".into());
        format!("udp://{host}:{port}")
    });

    info!("UDP public URL: {udp_public_url}");

    let st = AppState {
        core: Arc::new(Mutex::new(CoreState::default())),
        udp_public_url,
        udp_socket: None, // Will be set when UDP server starts
    };

    // Spawn UDP server
    let udp_state = st.clone();
    tokio::spawn(async move {
        if let Err(e) = run_udp_server(udp_state, udp_bind_addr).await {
            eprintln!("UDP server error: {e:#}");
        }
    });

    let svc = ControlPlaneSvc { st };
    info!("control gRPC listening on {grpc_addr}");

    tonic::transport::Server::builder()
        .add_service(ControlPlaneServer::new(svc))
        .serve(grpc_addr)
        .await?;

    Ok(())
}
