use async_trait::async_trait;

use crate::connection::HttpConnection;
use crate::h1::protocol::H1;
use crate::stream::{create_stream, TransportStream};
use crate::types::{ClientTimeouts, ProtocolError, Response};
use crate::utils::{parse_target, timeout_result};

/// Options required to establish an HTTP/1.1 connection.
#[derive(Debug, Clone)]
pub struct H1ConnectOptions {
    pub target: String,
    pub timeouts: ClientTimeouts,
}

/// Options required to read a response on an HTTP/1.1 connection.
#[derive(Debug, Clone, Copy)]
pub struct H1ReadOptions {
    pub read_body: bool,
}

pub struct H1Connection {
    client: H1,
    stream: TransportStream,
}

impl H1Connection {
    pub fn client(&self) -> &H1 {
        &self.client
    }

    pub fn client_mut(&mut self) -> &mut H1 {
        &mut self.client
    }

    pub fn stream_mut(&mut self) -> &mut TransportStream {
        &mut self.stream
    }
}

#[async_trait(?Send)]
impl HttpConnection for H1Connection {
    type ConnectOptions = H1ConnectOptions;
    type ReadOptions = H1ReadOptions;

    async fn connect(options: Self::ConnectOptions) -> Result<Self, ProtocolError> {
        let H1ConnectOptions { target, timeouts } = options;

        let parsed_target = parse_target(&target)?;
        let host = parsed_target
            .host()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing host".to_string()))?;
        let port = parsed_target
            .port()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing port".to_string()))?;
        let scheme = parsed_target.scheme().to_string();
        let host_owned = host.to_string();
        let connect_timeout = timeouts.connect;

        let stream = timeout_result(connect_timeout, async move {
            create_stream(&scheme, &host_owned, port, connect_timeout)
                .await
                .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))
        })
        .await?;

        Ok(Self {
            client: H1::timeouts(timeouts),
            stream,
        })
    }

    async fn read_response(
        &mut self,
        options: Self::ReadOptions,
    ) -> Result<Response, ProtocolError> {
        self.client
            .read_response(
                &mut self.stream,
                options.read_body,
                self.client.get_timeouts(),
            )
            .await
    }
}
