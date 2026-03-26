use crate::types::AppState;
use crate::utils::now_unix;
use anyhow::{anyhow, Result};

pub(crate) async fn consume_ticket(st: &AppState, token: &str) -> Result<(String, String, String)> {
    let mut core = st.core.lock().await;
    let t = core
        .tickets
        .remove(token)
        .ok_or_else(|| anyhow!("invalid_token"))?;
    if t.expires_at_unix < now_unix() {
        return Err(anyhow!("token_expired"));
    }
    Ok((t.match_id, t.player_id, t.display_name))
}
