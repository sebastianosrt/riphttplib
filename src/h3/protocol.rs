use crate::h3::connection::H3Connection;
use crate::types::{
    ClientTimeouts, FrameTypeH3, H3StreamErrorKind, Header, Protocol, ProtocolError, Request,
    Response,
};
use crate::utils::timeout_result;
use crate::PreparedRequest;
use async_trait::async_trait;
use bytes::Bytes;

#[derive(Clone)]
pub struct H3 {
    timeouts: ClientTimeouts,
}

impl H3 {
    pub fn new() -> Self {
        Self::timeouts(ClientTimeouts::default())
    }

    pub fn timeouts(timeouts: ClientTimeouts) -> Self {
        Self { timeouts }
    }

    pub fn get_timeouts(&self) -> &ClientTimeouts {
        &self.timeouts
    }

    pub fn session(&self) -> crate::session::H3Session {
        crate::session::H3Session::new(self.clone())
    }

    pub fn build_request(
        target: &str,
        method: impl Into<String>,
        headers: Vec<Header>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<PreparedRequest, ProtocolError> {
        let mut request = Request::new(target, method)?;
        request.headers_mut(headers);
        if let Some(body_bytes) = body {
            request.set_body(body_bytes);
        }
        if let Some(trailer_headers) = trailers {
            request.trailers_mut(trailer_headers);
        }
        request.prepare_request()
    }

    async fn send_request_inner(
        &self,
        connection: &mut H3Connection,
        request: &Request,
        timeouts: &ClientTimeouts,
    ) -> Result<u32, ProtocolError> {
        let (stream_id, mut send_stream) =
            timeout_result(timeouts.connect, connection.create_request_stream()).await?;

        let prepared = request.prepare_request()?;
        let header_block_entries = prepared.header_block();

        let header_block = timeout_result(
            timeouts.write,
            connection.encode_headers(stream_id, &header_block_entries),
        )
        .await?;
        let headers_frame =
            crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, header_block);
        let serialized_headers = headers_frame.serialize().map_err(|e| {
            ProtocolError::H3MessageError(format!("Failed to serialize headers: {}", e))
        })?;
        timeout_result(timeouts.write, async {
            send_stream
                .write_all(&serialized_headers)
                .await
                .map_err(|e| {
                    ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                        "Failed to send headers: {}",
                        e
                    )))
                })
        })
        .await?;

        if let Some(body) = prepared.body.as_ref() {
            if !body.is_empty() {
                let data_frame = crate::types::FrameH3::data(stream_id, body.clone());
                let serialized_data = data_frame.serialize().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize data: {}", e))
                })?;
                timeout_result(timeouts.write, async {
                    send_stream.write_all(&serialized_data).await.map_err(|e| {
                        ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                            "Failed to send data: {}",
                            e
                        )))
                    })
                })
                .await?;
            }
        }

        if !prepared.trailers.is_empty() {
            let trailer_block = timeout_result(
                timeouts.write,
                connection.encode_headers(stream_id, &prepared.trailers),
            )
            .await?;
            let trailers_frame =
                crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, trailer_block);
            let serialized_trailers = trailers_frame.serialize().map_err(|e| {
                ProtocolError::H3MessageError(format!("Failed to serialize trailers: {}", e))
            })?;
            timeout_result(timeouts.write, async {
                send_stream
                    .write_all(&serialized_trailers)
                    .await
                    .map_err(|e| {
                        ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                            "Failed to send trailers: {}",
                            e
                        )))
                    })
            })
            .await?;
        }

        timeout_result(timeouts.write, async {
            send_stream.finish().map_err(|e| {
                ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                    "Failed to finish stream: {}",
                    e
                )))
            })
        })
        .await?;

        Ok(stream_id)
    }

    pub async fn send_request(&self, request: Request) -> Result<Response, ProtocolError> {
        <Self as Protocol>::response(self, request).await
    }

    async fn perform_request(&self, request: &Request) -> Result<Response, ProtocolError> {
        let timeouts = request.timeouts(&self.timeouts);
        let connect_timeouts = timeouts.clone();
        let mut connection = timeout_result(
            timeouts.connect,
            H3Connection::connect_with_target_and_timeouts(&request.target, connect_timeouts),
        )
        .await?;
        let stream_id = self
            .send_request_inner(&mut connection, request, &timeouts)
            .await?;
        self.read_response(&mut connection, stream_id, &timeouts)
            .await
    }

    async fn read_response(
        &self,
        connection: &mut H3Connection,
        stream_id: u32,
        timeouts: &ClientTimeouts,
    ) -> Result<Response, ProtocolError> {
        connection
            .read_response_with_timeouts(stream_id, timeouts, None)
            .await
    }
}

#[async_trait(?Send)]
impl Protocol for H3 {
    async fn execute(&self, request: &Request) -> Result<Response, ProtocolError> {
        self.perform_request(request).await
    }
}
