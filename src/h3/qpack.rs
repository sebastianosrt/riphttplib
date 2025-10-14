use std::sync::Arc;

use bytes::Bytes;
use ls_qpack_rs::{
    decoder::{Decoder, DecoderError, DecoderOutput},
    encoder::{Encoder, EncoderError},
    StreamId,
};
use quinn::SendStream;
use tokio::sync::Mutex;

use crate::types::{H3StreamErrorKind, Header, ProtocolError};

fn map_encoder_err(_: EncoderError) -> ProtocolError {
    ProtocolError::H3QpackError("QPACK encode error".to_string())
}

fn map_decoder_err(_: DecoderError) -> ProtocolError {
    ProtocolError::H3QpackError("QPACK decode error".to_string())
}

fn map_stream_err(err: quinn::WriteError) -> ProtocolError {
    ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
        "QPACK stream write failed: {}",
        err
    )))
}

fn clamp_to_u32(value: u64) -> u32 {
    if value > u32::MAX as u64 {
        u32::MAX
    } else {
        value as u32
    }
}

/// Shared QPACK state bound to a single HTTP/3 connection.
#[derive(Clone)]
pub struct SharedQpackState(pub Arc<QpackState>);

impl SharedQpackState {
    pub fn new(local_table_capacity: u64, local_blocked_streams: u64) -> Self {
        SharedQpackState(Arc::new(QpackState::new(
            clamp_to_u32(local_table_capacity),
            clamp_to_u32(local_blocked_streams),
        )))
    }

    pub async fn set_encoder_send(&self, stream: SendStream) {
        self.0.set_encoder_send(stream).await;
    }

    pub async fn set_decoder_send(&self, stream: SendStream) {
        self.0.set_decoder_send(stream).await;
    }

    pub async fn configure_encoder(
        &self,
        max_table_capacity: u64,
        max_blocked_streams: u64,
    ) -> Result<(), ProtocolError> {
        self.0
            .configure_encoder(
                clamp_to_u32(max_table_capacity),
                clamp_to_u32(max_table_capacity),
                clamp_to_u32(max_blocked_streams),
            )
            .await
    }

    pub async fn encode_headers(
        &self,
        stream_id: u64,
        headers: &[Header],
    ) -> Result<Bytes, ProtocolError> {
        self.0.encode_headers(stream_id, headers).await
    }

    pub async fn decode_headers(
        &self,
        stream_id: u64,
        payload: &[u8],
    ) -> Result<QpackDecodeStatus, ProtocolError> {
        self.0.decode_headers(stream_id, payload).await
    }

    pub async fn poll_unblocked(
        &self,
        stream_id: u64,
    ) -> Result<Option<QpackDecodeStatus>, ProtocolError> {
        self.0.poll_unblocked(stream_id).await
    }

    pub async fn handle_encoder_stream_bytes(&self, bytes: Bytes) -> Result<(), ProtocolError> {
        self.0.handle_encoder_stream_bytes(bytes).await
    }

    pub async fn handle_decoder_stream_bytes(&self, bytes: Bytes) -> Result<(), ProtocolError> {
        self.0.handle_decoder_stream_bytes(bytes).await
    }
}

pub enum QpackDecodeStatus {
    Complete(Vec<Header>),
    Blocked,
}

pub struct QpackState {
    encoder: Mutex<Encoder>,
    decoder: Mutex<Decoder>,
    encoder_send: Mutex<Option<SendStream>>,
    decoder_send: Mutex<Option<SendStream>>,
}

impl QpackState {
    fn new(local_table_capacity: u32, local_blocked_streams: u32) -> Self {
        Self {
            encoder: Mutex::new(Encoder::new()),
            decoder: Mutex::new(Decoder::new(local_table_capacity, local_blocked_streams)),
            encoder_send: Mutex::new(None),
            decoder_send: Mutex::new(None),
        }
    }

    async fn set_encoder_send(&self, stream: SendStream) {
        let mut guard = self.encoder_send.lock().await;
        *guard = Some(stream);
    }

    async fn set_decoder_send(&self, stream: SendStream) {
        let mut guard = self.decoder_send.lock().await;
        *guard = Some(stream);
    }

    async fn write_encoder_stream(&self, data: &[u8]) -> Result<(), ProtocolError> {
        if data.is_empty() {
            return Ok(());
        }

        let mut guard = self.encoder_send.lock().await;
        if let Some(stream) = guard.as_mut() {
            stream.write_all(data).await.map_err(map_stream_err)?;
        }
        Ok(())
    }

    async fn write_decoder_stream(&self, data: &[u8]) -> Result<(), ProtocolError> {
        if data.is_empty() {
            return Ok(());
        }

        let mut guard = self.decoder_send.lock().await;
        if let Some(stream) = guard.as_mut() {
            stream.write_all(data).await.map_err(map_stream_err)?;
        }
        Ok(())
    }

    async fn configure_encoder(
        &self,
        max_table_size: u32,
        dyn_table_size: u32,
        max_blocked_streams: u32,
    ) -> Result<(), ProtocolError> {
        let mut encoder = self.encoder.lock().await;
        let sdtc = encoder
            .configure(max_table_size, dyn_table_size, max_blocked_streams)
            .map_err(map_encoder_err)?;
        drop(encoder);

        self.write_encoder_stream(sdtc.as_ref()).await
    }

    async fn encode_headers(
        &self,
        stream_id: u64,
        headers: &[Header],
    ) -> Result<Bytes, ProtocolError> {
        let tuples: Vec<(String, String)> = headers
            .iter()
            .map(|h| {
                let value = h.value.clone().unwrap_or_else(|| "".to_string());
                (h.name.clone(), value)
            })
            .collect();

        let mut encoder = self.encoder.lock().await;
        let buffers = encoder
            .encode_all(
                StreamId::new(stream_id),
                tuples
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )
            .map_err(map_encoder_err)?;
        let (header_block, encoder_stream) = buffers.into();
        drop(encoder);

        self.write_encoder_stream(&encoder_stream).await?;
        Ok(Bytes::from(Vec::from(header_block)))
    }

    async fn decode_headers(
        &self,
        stream_id: u64,
        payload: &[u8],
    ) -> Result<QpackDecodeStatus, ProtocolError> {
        let mut decoder = self.decoder.lock().await;
        let output = decoder
            .decode(StreamId::new(stream_id), payload.to_vec())
            .map_err(map_decoder_err)?;

        match output {
            DecoderOutput::Done(decoded) => {
                let ack = decoded.stream().to_vec();
                let headers = decoded
                    .headers()
                    .iter()
                    .map(|header| Header {
                        name: header.name().to_string(),
                        value: Some(header.value().to_string()),
                    })
                    .collect::<Vec<_>>();

                drop(decoder);
                self.write_decoder_stream(&ack).await?;
                Ok(QpackDecodeStatus::Complete(headers))
            }
            DecoderOutput::BlockedStream => Ok(QpackDecodeStatus::Blocked),
        }
    }

    async fn poll_unblocked(
        &self,
        stream_id: u64,
    ) -> Result<Option<QpackDecodeStatus>, ProtocolError> {
        let mut decoder = self.decoder.lock().await;
        let result = decoder.unblocked(StreamId::new(stream_id));
        match result {
            Some(Ok(DecoderOutput::Done(decoded))) => {
                let ack = decoded.stream().to_vec();
                let headers = decoded
                    .headers()
                    .iter()
                    .map(|header| Header {
                        name: header.name().to_string(),
                        value: Some(header.value().to_string()),
                    })
                    .collect::<Vec<_>>();
                drop(decoder);
                self.write_decoder_stream(&ack).await?;
                Ok(Some(QpackDecodeStatus::Complete(headers)))
            }
            Some(Ok(DecoderOutput::BlockedStream)) => Ok(Some(QpackDecodeStatus::Blocked)),
            Some(Err(e)) => Err(map_decoder_err(e)),
            None => Ok(None),
        }
    }

    async fn handle_encoder_stream_bytes(&self, bytes: Bytes) -> Result<(), ProtocolError> {
        if bytes.is_empty() {
            return Ok(());
        }

        let mut decoder = self.decoder.lock().await;
        decoder.feed(bytes.as_ref()).map_err(map_decoder_err)
    }

    async fn handle_decoder_stream_bytes(&self, bytes: Bytes) -> Result<(), ProtocolError> {
        if bytes.is_empty() {
            return Ok(());
        }

        let mut encoder = self.encoder.lock().await;
        encoder.feed(bytes.as_ref()).map_err(map_encoder_err)
    }
}
