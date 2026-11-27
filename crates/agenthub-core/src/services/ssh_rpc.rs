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

pub async fn write_raw_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    id: u64,
    result: Option<&serde_json::value::RawValue>,
    error: Option<&str>,
) -> Result<()> {
    if let (Some(raw), None) = (result, error) {
        let prefix = format!("{{\"id\":{id},\"result\":");
        let body = raw.get().as_bytes();
        let suffix = b",\"error\":null}";
        let total = prefix
            .len()
            .saturating_add(body.len())
            .saturating_add(suffix.len());
        ensure!(
            total <= crate::resource_limits::REMOTE_RESPONSE_BYTES,
            "RPC frame exceeds size limit"
        );
        writer.write_all(&(total as u32).to_be_bytes()).await?;
        writer.write_all(prefix.as_bytes()).await?;
        writer.write_all(body).await?;
        writer.write_all(suffix).await?;
        writer.flush().await?;
        return Ok(());
    }
    write(
        writer,
        &Response {
            id,
            result: result.and_then(|r| serde_json::from_str(r.get()).ok()),
            error: error.map(str::to_owned),
        },
    )
    .await
}

pub async fn read_raw_response<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<
    Option<(
        u64,
        Option<Box<serde_json::value::RawValue>>,
        Option<String>,
    )>,
> {
    #[derive(Deserialize)]
    struct RawResponseFrame {
        id: u64,
        result: Option<Box<serde_json::value::RawValue>>,
        error: Option<String>,
    }
    let Some(frame) = read::<_, RawResponseFrame>(reader).await? else {
        return Ok(None);
    };
    Ok(Some((frame.id, frame.result, frame.error)))
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

        let raw_payload =
            serde_json::value::RawValue::from_string("{\"items\":[1,2,3]}".into()).unwrap();
        let mut raw_frame = Vec::new();
        write_raw_response(&mut raw_frame, 42, Some(&raw_payload), None)
            .await
            .unwrap();
        let decoded: Response = read(&mut raw_frame.as_slice()).await.unwrap().unwrap();
        assert_eq!(decoded.id, 42);
        assert_eq!(
            decoded.result,
            Some(serde_json::json!({"items": [1, 2, 3]}))
        );
        assert_eq!(decoded.error, None);
        let (rid, rval, rerr) = read_raw_response(&mut raw_frame.as_slice())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rid, 42);
        assert_eq!(rval.unwrap().get(), "{\"items\":[1,2,3]}");
        assert_eq!(rerr, None);
    }
}
