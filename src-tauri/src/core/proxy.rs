use crate::error::{AppError, AppResult};
use reqwest::Client;
use bytes::Bytes;
use futures_util::StreamExt;

/// Proxy a request to the upstream provider
/// Returns the raw response for streaming back to client
pub async fn proxy_request(
    client: &Client,
    url: &str,
    method: reqwest::Method,
    headers: reqwest::header::HeaderMap,
    body: Bytes,
) -> AppResult<reqwest::Response> {
    let mut req = client.request(method, url).body(body);

    // Forward selected headers (Authorization, Content-Type, etc.)
    for (name, value) in headers.iter() {
        req = req.header(name, value);
    }

    let response = req.send().await.map_err(|e| AppError::Proxy(e.to_string()))?;
    Ok(response)
}

/// Extract SSE streaming body from upstream response
/// Returns a stream of bytes to forward to the client
pub async fn stream_sse(
    response: reqwest::Response,
) -> AppResult<impl tokio_stream::Stream<Item = Result<Bytes, std::io::Error>>> {
    let stream = response.bytes_stream().map(|chunk| {
        chunk.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });
    Ok(stream)
}
