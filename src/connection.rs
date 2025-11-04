use async_trait::async_trait;

use crate::types::{ProtocolError, Response};

/// Common abstraction for raw HTTP connection types (H1/H2/H3).
///
/// Implementors expose a uniform way to establish a connection and read a full
/// `Response` while allowing protocol-specific parameters through associated
/// option types.
#[async_trait(?Send)]
pub trait HttpConnection: Sized {
    /// Parameters required to create the connection.
    type ConnectOptions: Send;

    /// Parameters required to read a response on the connection.
    type ReadOptions: Send;

    /// Establish a connection using the provided options.
    async fn connect(options: Self::ConnectOptions) -> Result<Self, ProtocolError>;

    /// Read a response using the provided options.
    async fn read_response(
        &mut self,
        options: Self::ReadOptions,
    ) -> Result<Response, ProtocolError>;
}
