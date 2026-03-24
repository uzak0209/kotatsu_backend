use anyhow::{anyhow, Context, Result};
use quinn::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

pub(crate) async fn read_json_line<T: for<'de> Deserialize<'de>>(
    recv: &mut RecvStream,
    buf: &mut Vec<u8>,
) -> Result<T> {
    loop {
        if let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line = buf.drain(..=pos).collect::<Vec<u8>>();
            let line = &line[..line.len() - 1];
            if line.is_empty() {
                continue;
            }
            let msg = serde_json::from_slice::<T>(line).context("parse json line")?;
            return Ok(msg);
        }
        let chunk = recv
            .read_chunk(4096, true)
            .await
            .context("read reliable chunk")?;
        match chunk {
            Some(c) => buf.extend_from_slice(&c.bytes),
            None => return Err(anyhow!("stream closed")),
        }
    }
}

pub(crate) async fn write_json_line<T: Serialize>(send: &mut SendStream, msg: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(msg)?;
    bytes.push(b'\n');
    send.write_all(&bytes).await?;
    Ok(())
}
