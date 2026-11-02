use super::{DesktopError, Result};
use crate::resource_limits::CODEX_DESKTOP_FRAME_BYTES;
use serde_json::Value;
use std::io::{self, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Default)]
pub(super) struct FrameReader {
    header: [u8; 4],
    header_read: usize,
    body: Vec<u8>,
    body_read: usize,
}

impl FrameReader {
    // Keep read offsets on the reader, not in a cancellable read_exact future.
    pub(super) async fn read(&mut self, stream: &mut (impl AsyncRead + Unpin)) -> Result<Value> {
        while self.header_read < self.header.len() {
            let count = stream
                .read(&mut self.header[self.header_read..])
                .await
                .map_err(|_| DesktopError::Disconnected)?;
            if count == 0 {
                return Err(DesktopError::Disconnected);
            }
            self.header_read += count;
        }
        let size = u32::from_le_bytes(self.header) as usize;
        if size == 0 {
            return Err(DesktopError::InvalidFrame);
        }
        if size > CODEX_DESKTOP_FRAME_BYTES {
            return Err(DesktopError::FrameTooLarge);
        }
        if self.body.is_empty() {
            self.body.resize(size, 0);
        }
        while self.body_read < size {
            let count = stream
                .read(&mut self.body[self.body_read..])
                .await
                .map_err(|_| DesktopError::Disconnected)?;
            if count == 0 {
                return Err(DesktopError::Disconnected);
            }
            self.body_read += count;
        }
        let value =
            serde_json::from_slice::<Value>(&self.body).map_err(|_| DesktopError::InvalidFrame)?;
        if !value.is_object() {
            return Err(DesktopError::InvalidFrame);
        }
        self.header_read = 0;
        self.body_read = 0;
        // Do not pin a 64 MiB allocation after a large initial snapshot.
        self.body = Vec::new();
        Ok(value)
    }
}

struct BoundedWriter(Vec<u8>);

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > CODEX_DESKTOP_FRAME_BYTES.saturating_sub(self.0.len()) {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct FrameWriter {
    pending: Vec<u8>,
    written: usize,
}

impl FrameWriter {
    pub(super) async fn send(
        &mut self,
        stream: &mut (impl AsyncWrite + Unpin),
        value: &Value,
    ) -> Result<()> {
        self.flush(stream).await?;
        let mut encoded = BoundedWriter(Vec::new());
        serde_json::to_writer(&mut encoded, value).map_err(|_| DesktopError::FrameTooLarge)?;
        self.pending = Vec::with_capacity(4 + encoded.0.len());
        self.pending
            .extend_from_slice(&(encoded.0.len() as u32).to_le_bytes());
        self.pending.extend_from_slice(&encoded.0);
        self.flush(stream).await
    }

    pub(super) async fn flush(&mut self, stream: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        while self.written < self.pending.len() {
            let count = stream
                .write(&self.pending[self.written..])
                .await
                .map_err(|_| DesktopError::Disconnected)?;
            if count == 0 {
                return Err(DesktopError::Disconnected);
            }
            self.written += count;
        }
        stream
            .flush()
            .await
            .map_err(|_| DesktopError::Disconnected)?;
        self.pending = Vec::new();
        self.written = 0;
        Ok(())
    }
}

#[cfg(test)]
pub(super) async fn write_frame(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &Value,
) -> Result<()> {
    FrameWriter::default().send(stream, value).await
}
