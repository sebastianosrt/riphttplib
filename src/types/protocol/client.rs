use super::{ClientRequest, Protocol};
use crate::{H1, H2, H3};

pub trait DefaultClient: Protocol + Send + Unpin + 'static {
    fn default_client() -> Self;
}

impl DefaultClient for H1 {
    fn default_client() -> Self {
        H1::new()
    }
}

impl DefaultClient for H2 {
    fn default_client() -> Self {
        H2::new()
    }
}

impl DefaultClient for H3 {
    fn default_client() -> Self {
        H3::new()
    }
}

pub struct Client<C = H1>
where
    C: Protocol + DefaultClient + Send + Unpin + 'static,
{
    _marker: std::marker::PhantomData<C>,
}

impl<C> Client<C>
where
    C: Protocol + DefaultClient + Send + Unpin + 'static,
{
    pub fn request(method: &str, url: &str) -> ClientRequest<C> {
        ClientRequest::new(C::default_client(), method, url)
    }

    pub fn with(client: C) -> TypedClient<C> {
        TypedClient { client }
    }

    pub fn get(url: &str) -> ClientRequest<C> {
        Self::request("GET", url)
    }

    pub fn head(url: &str) -> ClientRequest<C> {
        Self::request("HEAD", url)
    }

    pub fn post(url: &str) -> ClientRequest<C> {
        Self::request("POST", url)
    }

    pub fn put(url: &str) -> ClientRequest<C> {
        Self::request("PUT", url)
    }

    pub fn patch(url: &str) -> ClientRequest<C> {
        Self::request("PATCH", url)
    }

    pub fn delete(url: &str) -> ClientRequest<C> {
        Self::request("DELETE", url)
    }

    pub fn options(url: &str) -> ClientRequest<C> {
        Self::request("OPTIONS", url)
    }

    pub fn trace(url: &str) -> ClientRequest<C> {
        Self::request("TRACE", url)
    }

    pub fn connect(url: &str) -> ClientRequest<C> {
        Self::request("CONNECT", url)
    }
}

pub struct TypedClient<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    client: C,
}

impl<C> TypedClient<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    pub fn request(&self, method: &str, url: &str) -> ClientRequest<C>
    where
        C: Clone,
    {
        ClientRequest::new(self.client.clone(), method, url)
    }

    pub fn get(&self, url: &str) -> ClientRequest<C>
    where
        C: Clone,
    {
        self.request("GET", url)
    }

    pub fn post(&self, url: &str) -> ClientRequest<C>
    where
        C: Clone,
    {
        self.request("POST", url)
    }
}
