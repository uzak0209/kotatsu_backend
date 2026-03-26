use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ParamKind {
    Gravity,
    Friction,
    Speed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ParamDirection {
    Increase,
    Decrease,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct PlayerParams {
    pub(crate) gravity: u8,
    pub(crate) friction: u8,
    pub(crate) speed: u8,
}

impl Default for PlayerParams {
    fn default() -> Self {
        Self {
            gravity: 2,
            friction: 2,
            speed: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ParamMutation {
    pub(crate) params: PlayerParams,
    pub(crate) next_param_change_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParamUpdateError {
    CooldownActive { next_allowed_at_unix: u64 },
    OutOfRange,
}

impl std::fmt::Display for ParamUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CooldownActive {
                next_allowed_at_unix,
            } => {
                write!(f, "cooldown_active:{next_allowed_at_unix}")
            }
            Self::OutOfRange => write!(f, "out_of_range"),
        }
    }
}

impl std::error::Error for ParamUpdateError {}

#[derive(Debug, Clone)]
pub(crate) struct Ticket {
    pub(crate) match_id: String,
    pub(crate) player_id: String,
    pub(crate) display_name: String,
    pub(crate) expires_at_unix: u64,
}

#[derive(Debug)]
pub(crate) enum PlayerConnection {
    Udp(SocketAddr),
}

#[derive(Debug)]
pub(crate) struct PlayerHandle {
    pub(crate) display_name: String,
    pub(crate) params: PlayerParams,
    pub(crate) next_param_change_at_unix: u64,
    pub(crate) reliable_tx: mpsc::Sender<ServerReliable>,
    pub(crate) connection: Option<PlayerConnection>,
}

#[derive(Debug)]
pub(crate) struct MatchRoom {
    pub(crate) players: HashMap<String, PlayerHandle>,
    pub(crate) started_at_unix: u64,
}

impl MatchRoom {
    pub(crate) fn new() -> Self {
        Self {
            players: HashMap::new(),
            started_at_unix: 0,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct CoreState {
    pub(crate) matches: HashMap<String, MatchRoom>,
    pub(crate) tickets: HashMap<String, Ticket>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) core: Arc<Mutex<CoreState>>,
    pub(crate) udp_public_url: String,
    pub(crate) udp_socket: Option<Arc<UdpSocket>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(crate) enum ClientReliable {
    Join {
        token: String,
    },
    ParamChange {
        seq: u64,
        param: ParamKind,
        direction: ParamDirection,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(crate) enum ServerReliable {
    JoinOk {
        match_id: String,
        player_id: String,
        params: PlayerParams,
        server_time_ms: u64,
    },
    MatchStarted {
        match_id: String,
        started_at_unix: u64,
        server_time_ms: u64,
    },
    ParamApplied {
        from_player_id: String,
        seq: u64,
        params: PlayerParams,
        next_param_change_at_unix: u64,
        server_time_ms: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(crate) enum ClientDatagram {
    Pos {
        seq: u64,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(crate) enum ServerDatagram {
    Pos {
        player_id: String,
        seq: u64,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        server_time_ms: u64,
    },
}
