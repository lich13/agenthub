//! Framed business RPC shared by the SSH client and the private service socket.
use anyhow::{ensure, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
pub const SOCKET_PATH: &str = "/run/agenthub-webd/rpc.sock";
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub command: String,
    pub args: Value,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<String>,
}
pub async fn read<R: AsyncRead + Unpin, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>> {
    let mut prefix = [0; 4];
    if reader.read(&mut prefix[..1]).await? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut prefix[1..]).await?;
    let length = u32::from_be_bytes(prefix) as usize;
    ensure!(
        length <= crate::resource_limits::REMOTE_RESPONSE_BYTES,
        "RPC frame exceeds size limit"
    );
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}
pub async fn write<W: AsyncWrite + Unpin, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= crate::resource_limits::REMOTE_RESPONSE_BYTES,
        "RPC frame exceeds size limit"
    );
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn frame_round_trip_and_truncation() {
        let mut bytes = Vec::new();
        write(
            &mut bytes,
            &Request {
                id: 7,
                command: "system.capabilities".into(),
                args: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
        let got: Request = read(&mut bytes.as_slice()).await.unwrap().unwrap();
        assert_eq!(got.id, 7);
        assert!(read::<_, Request>(&mut &bytes[..bytes.len() - 1])
            .await
            .is_err());
        assert!(read::<_, Request>(&mut &[][..]).await.unwrap().is_none());
    }
}
