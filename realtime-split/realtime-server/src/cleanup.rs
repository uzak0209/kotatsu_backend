use tokio::time::{sleep, Duration};
use tracing::info;

use crate::magicnums::{ROOM_CLEANUP_INTERVAL_SECS, ROOM_IDLE_TIMEOUT_SECS};
use crate::room::delete_stale_lobby_rooms;
use crate::types::AppState;

pub(crate) fn spawn_room_cleanup_task(st: AppState) {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(ROOM_CLEANUP_INTERVAL_SECS)).await;

            let deleted = delete_stale_lobby_rooms(&st, ROOM_IDLE_TIMEOUT_SECS).await;
            if !deleted.is_empty() {
                info!(
                    ?deleted,
                    timeout_secs = ROOM_IDLE_TIMEOUT_SECS,
                    "deleted stale lobby rooms"
                );
            }
        }
    });
}
