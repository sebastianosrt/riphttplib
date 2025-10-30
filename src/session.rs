use crate::h1::client::H1Client;
use crate::h2::client::H2Client;
use crate::h3::client::H3Client;
use crate::types::{ClientTimeouts, Header, ProtocolError, ProxySettings, Request, Response};
use bytes::Bytes;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Default)]
pub struct CookieStore {
    entries: BTreeMap<String, String>,
}

impl CookieStore {
    fn apply_to_request(&self, request: &mut Request) {
        if self.entries.is_empty() {
            return;
        }

        let mut combined: BTreeMap<String, String> = self.entries.clone();
        for (name, value) in &request.cookies {
            combined.insert(name.clone(), value.clone());
        }

        request.cookies = combined
            .into_iter()
            .map(|(name, value)| (name, value))
            .collect();
    }

    fn update_from_response(&mut self, response: &Response) {
        for (name, value) in &response.cookies {
            if value.is_empty() {
                self.entries.remove(name);
            } else {
                self.entries.insert(name.clone(), value.clone());
            }
        }
    }

    pub fn set_cookie(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(name.into(), value.into());
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.entries.iter()
    }
}

impl fmt::Display for CookieStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (name, value) in &self.entries {
            if !first {
                write!(f, "; ")?;
            }
            write!(f, "{}={}", name, value)?;
            first = false;
        }
        if first {
            write!(f, "<empty cookies>")?;
        }
        Ok(())
    }
}


/*
// TODO complete sessions
 __attrs__ = [
        "auth",
        "params",
        "verify",
        "cert",
        "stream",
        "trust_env",
        "max_redirects",
    ]
*/

#[derive(Debug, Clone, Default)]
pub struct SessionRequestOptions {
    pub headers: Option<Vec<Header>>,
    pub body: Option<Bytes>,
    pub trailers: Option<Vec<Header>>,
    pub cookies: Option<Vec<(String, String)>>,
    pub params: Option<Vec<(String, String)>>,
    pub follow_redirects: Option<bool>,
    pub timeout: Option<ClientTimeouts>,
    pub proxies: Option<ProxySettings>,
}

impl SessionRequestOptions {
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Self {
        Self {
            headers,
            body,
            trailers,
            cookies,
            params,
            follow_redirects,
            timeout,
            proxies,
        }
    }
}

fn build_request(
    method: &str,
    url: &str,
    options: &SessionRequestOptions,
) -> Result<Request, ProtocolError> {
    let mut request = Request::new(url, method)?;

    if let Some(headers) = &options.headers {
        request = request.headers(headers.clone());
    }

    if let Some(body) = &options.body {
        request = request.body(body.clone());
    }

    if let Some(trailers) = &options.trailers {
        request = request.trailers(trailers.clone());
    }

    if let Some(cookies) = &options.cookies {
        request = request.cookies(cookies.clone());
    }

    if let Some(params) = &options.params {
        request = request.params(params.clone());
    }

    if let Some(allow) = options.follow_redirects {
        request = request.follow_redirects(allow);
    }

    if let Some(timeout) = &options.timeout {
        request = request.timeout(timeout.clone());
    }

    if let Some(proxies) = &options.proxies {
        request = request.proxies(proxies.clone());
    }

    Ok(request)
}

fn apply_default_headers(defaults: &[Header], request: &mut Request) {
    for header in defaults {
        let exists = request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(&header.name));
        if !exists {
            request.headers.push(header.clone());
        }
    }
}

/// Shared behaviour for HTTP sessions.
trait SessionInner {
    fn default_headers(&self) -> &Vec<Header>;
    fn default_headers_mut(&mut self) -> &mut Vec<Header>;
    fn cookie_store(&self) -> &CookieStore;
    fn cookie_store_mut(&mut self) -> &mut CookieStore;

    fn prepare_request(&self, request: &mut Request) {
        apply_default_headers(self.default_headers(), request);
        self.cookie_store().apply_to_request(request);
    }

    fn finalize_response(&mut self, response: &Response) {
        self.cookie_store_mut().update_from_response(response);
    }

    fn add_default_header(&mut self, header: Header) {
        let exists = self
            .default_headers()
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(&header.name));
        if !exists {
            self.default_headers_mut().push(header);
        }
    }

    fn set_cookie(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.cookie_store_mut().set_cookie(name, value);
    }
}

pub struct H1Session {
    client: H1Client,
    default_headers: Vec<Header>,
    pub cookies: CookieStore,
}

impl H1Session {
    pub(crate) fn new(client: H1Client) -> Self {
        Self {
            client,
            default_headers: Vec::new(),
            cookies: CookieStore::default(),
        }
    }

    pub fn add_default_header(&mut self, header: Header) {
        SessionInner::add_default_header(self, header);
    }

    pub fn set_cookie(&mut self, name: impl Into<String>, value: impl Into<String>) {
        SessionInner::set_cookie(self, name, value);
    }

    pub async fn send(&mut self, mut request: Request) -> Result<Response, ProtocolError> {
        SessionInner::prepare_request(self, &mut request);
        let response = self.client.send_request(request).await?;
        SessionInner::finalize_response(self, &response);
        Ok(response)
    }

    pub async fn request(
        &mut self,
        method: &str,
        url: &str,
        options: SessionRequestOptions,
    ) -> Result<Response, ProtocolError> {
        let request = build_request(method, url, &options)?;
        self.send(request).await
    }

    async fn request_with_parts(
        &mut self,
        method: &str,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        let options = SessionRequestOptions::from_parts(
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        );
        self.request(method, url, options).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "GET",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn post(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "POST",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn put(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "PUT",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "DELETE",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn patch(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "PATCH",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn head(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "HEAD",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn options(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "OPTIONS",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    pub fn client(&self) -> &H1Client {
        &self.client
    }
}

impl SessionInner for H1Session {
    fn default_headers(&self) -> &Vec<Header> {
        &self.default_headers
    }

    fn default_headers_mut(&mut self) -> &mut Vec<Header> {
        &mut self.default_headers
    }

    fn cookie_store(&self) -> &CookieStore {
        &self.cookies
    }

    fn cookie_store_mut(&mut self) -> &mut CookieStore {
        &mut self.cookies
    }
}

pub struct H2Session {
    client: H2Client,
    default_headers: Vec<Header>,
    pub cookies: CookieStore,
}

impl H2Session {
    pub(crate) fn new(client: H2Client) -> Self {
        Self {
            client,
            default_headers: Vec::new(),
            cookies: CookieStore::default(),
        }
    }

    pub fn add_default_header(&mut self, header: Header) {
        SessionInner::add_default_header(self, header);
    }

    pub fn set_cookie(&mut self, name: impl Into<String>, value: impl Into<String>) {
        SessionInner::set_cookie(self, name, value);
    }

    pub async fn send(&mut self, mut request: Request) -> Result<Response, ProtocolError> {
        SessionInner::prepare_request(self, &mut request);
        let response = self.client.send_request(request).await?;
        SessionInner::finalize_response(self, &response);
        Ok(response)
    }

    pub async fn request(
        &mut self,
        method: &str,
        url: &str,
        options: SessionRequestOptions,
    ) -> Result<Response, ProtocolError> {
        let request = build_request(method, url, &options)?;
        self.send(request).await
    }

    async fn request_with_parts(
        &mut self,
        method: &str,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        let options = SessionRequestOptions::from_parts(
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        );
        self.request(method, url, options).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "GET",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn post(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "POST",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn put(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "PUT",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "DELETE",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn patch(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "PATCH",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn head(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "HEAD",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn options(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "OPTIONS",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    pub fn client(&self) -> &H2Client {
        &self.client
    }
}

impl SessionInner for H2Session {
    fn default_headers(&self) -> &Vec<Header> {
        &self.default_headers
    }

    fn default_headers_mut(&mut self) -> &mut Vec<Header> {
        &mut self.default_headers
    }

    fn cookie_store(&self) -> &CookieStore {
        &self.cookies
    }

    fn cookie_store_mut(&mut self) -> &mut CookieStore {
        &mut self.cookies
    }
}

pub struct H3Session {
    client: H3Client,
    default_headers: Vec<Header>,
    pub cookies: CookieStore,
}

impl H3Session {
    pub(crate) fn new(client: H3Client) -> Self {
        Self {
            client,
            default_headers: Vec::new(),
            cookies: CookieStore::default(),
        }
    }

    pub fn add_default_header(&mut self, header: Header) {
        SessionInner::add_default_header(self, header);
    }

    pub fn set_cookie(&mut self, name: impl Into<String>, value: impl Into<String>) {
        SessionInner::set_cookie(self, name, value);
    }

    pub async fn send(&mut self, mut request: Request) -> Result<Response, ProtocolError> {
        SessionInner::prepare_request(self, &mut request);
        let response = self.client.send_request(request).await?;
        SessionInner::finalize_response(self, &response);
        Ok(response)
    }

    pub async fn request(
        &mut self,
        method: &str,
        url: &str,
        options: SessionRequestOptions,
    ) -> Result<Response, ProtocolError> {
        let request = build_request(method, url, &options)?;
        self.send(request).await
    }

    async fn request_with_parts(
        &mut self,
        method: &str,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        let options = SessionRequestOptions::from_parts(
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        );
        self.request(method, url, options).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "GET",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn post(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "POST",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn put(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "PUT",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "DELETE",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn patch(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "PATCH",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn head(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "HEAD",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn options(
        &mut self,
        url: &str,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
        cookies: Option<Vec<(String, String)>>,
        follow_redirects: Option<bool>,
        params: Option<Vec<(String, String)>>,
        timeout: Option<ClientTimeouts>,
        proxies: Option<ProxySettings>,
    ) -> Result<Response, ProtocolError> {
        self.request_with_parts(
            "OPTIONS",
            url,
            headers,
            body,
            trailers,
            cookies,
            follow_redirects,
            params,
            timeout,
            proxies,
        )
        .await
    }

    pub fn client(&self) -> &H3Client {
        &self.client
    }
}

impl SessionInner for H3Session {
    fn default_headers(&self) -> &Vec<Header> {
        &self.default_headers
    }

    fn default_headers_mut(&mut self) -> &mut Vec<Header> {
        &mut self.default_headers
    }

    fn cookie_store(&self) -> &CookieStore {
        &self.cookies
    }

    fn cookie_store_mut(&mut self) -> &mut CookieStore {
        &mut self.cookies
    }
}
