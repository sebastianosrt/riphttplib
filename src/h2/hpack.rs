use bytes::Bytes;
use hpack::{Decoder, Encoder};

use crate::types::{Header, ProtocolError};

pub struct HpackCodec {
    encoder: Encoder<'static>,
    decoder: Decoder<'static>,
    encoder_max_table_size: usize,
    decoder_max_table_size: usize,
}

impl HpackCodec {
    pub fn new(encoder_max_table_size: usize, decoder_max_table_size: usize) -> Self {
        let mut codec = Self {
            encoder: Encoder::new(),
            decoder: Decoder::new(),
            encoder_max_table_size,
            decoder_max_table_size,
        };
        codec.apply_decoder_table_size(decoder_max_table_size);
        codec
    }

    pub fn set_encoder_max_table_size(&mut self, size: usize) {
        self.encoder_max_table_size = size;
        // The hpack encoder crate does not expose an API to bound the dynamic
        // table size directly; this value is tracked so we can emit SETTINGS
        // updates when necessary.
    }

    pub fn set_decoder_max_table_size(&mut self, size: usize) {
        self.decoder_max_table_size = size;
        self.apply_decoder_table_size(size);
    }

    pub fn encoder_max_table_size(&self) -> usize {
        self.encoder_max_table_size
    }

    pub fn decoder_max_table_size(&self) -> usize {
        self.decoder_max_table_size
    }

    pub fn encode(&mut self, headers: &[Header]) -> Result<Bytes, ProtocolError> {
        let header_tuples = headers
            .iter()
            .map(|h| {
                let name = h.name.as_bytes();
                let value = h.value.as_ref().map(|v| v.as_bytes()).unwrap_or(&[]);
                (name, value)
            })
            .collect::<Vec<_>>();

        let encoded = self.encoder.encode(header_tuples);
        Ok(Bytes::from(encoded))
    }

    pub fn decode(&mut self, payload: &[u8]) -> Result<Vec<Header>, ProtocolError> {
        match self.decoder.decode(payload) {
            Ok(entries) => entries
                .into_iter()
                .map(|(name, value)| Self::into_header(name, value))
                .collect(),
            Err(err) => Err(ProtocolError::H2CompressionError(format!(
                "HPACK decode error: {:?}",
                err
            ))),
        }
    }

    fn into_header(name: Vec<u8>, value: Vec<u8>) -> Result<Header, ProtocolError> {
        let name_str = String::from_utf8(name).map_err(|e| {
            ProtocolError::HeaderEncodingError(format!("Invalid UTF-8 in header name: {}", e))
        })?;
        let value = if value.is_empty() {
            None
        } else {
            Some(String::from_utf8(value).map_err(|e| {
                ProtocolError::HeaderEncodingError(format!("Invalid UTF-8 in header value: {}", e))
            })?)
        };

        Ok(Header {
            name: name_str,
            value,
        })
    }

    fn apply_decoder_table_size(&mut self, size: usize) {
        self.decoder.set_max_table_size(size);
    }
}
