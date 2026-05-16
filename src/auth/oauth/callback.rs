use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use crate::auth::errors::OAuthError;

pub const DEFAULT_CALLBACK_PORT: u16 = 53682;
pub const CALLBACK_PATH: &str = "/callback";

#[derive(Debug, Clone)]
pub struct CallbackResult {
    pub code: String,
    pub state: String,
}

pub struct LoopbackReceiver {
    listener: TcpListener,
    port: u16,
}

impl LoopbackReceiver {
    pub async fn bind(port: u16) -> Result<Self, OAuthError> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|_| OAuthError::PortInUse { port })?;
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}{}", self.port, CALLBACK_PATH)
    }

    pub async fn accept_once(self, timeout: Duration) -> Result<CallbackResult, OAuthError> {
        let (mut stream, _) = tokio::time::timeout(timeout, self.listener.accept())
            .await
            .map_err(|_| OAuthError::MalformedCallback("timed out waiting for callback".into()))?
            .map_err(OAuthError::Io)?;

        let mut buf = vec![0u8; 8192];
        let mut total = 0usize;
        loop {
            let n = stream.read(&mut buf[total..]).await?;
            if n == 0 {
                break;
            }
            total += n;
            if let Some(end) = find_double_crlf(&buf[..total]) {
                buf.truncate(end);
                break;
            }
            if total >= buf.len() {
                return Err(OAuthError::MalformedCallback(
                    "request line exceeded buffer".into(),
                ));
            }
        }

        let request = std::str::from_utf8(&buf)
            .map_err(|_| OAuthError::MalformedCallback("non-utf8 callback".into()))?;
        let request_line = request
            .lines()
            .next()
            .ok_or_else(|| OAuthError::MalformedCallback("empty callback request".into()))?;

        let parsed = parse_request_line(request_line)?;

        let response = if parsed.code.is_some() {
            success_response()
        } else if let Some(err) = &parsed.error {
            error_response(err)
        } else {
            error_response("missing_code")
        };
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;

        match parsed {
            ParsedCallback {
                code: Some(code),
                state: Some(state),
                ..
            } => Ok(CallbackResult { code, state }),
            ParsedCallback {
                error: Some(err), ..
            } => Err(OAuthError::AuthorizationDenied(err)),
            _ => Err(OAuthError::MalformedCallback(
                "callback missing code or state".into(),
            )),
        }
    }
}

#[derive(Debug, Default)]
struct ParsedCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn parse_request_line(line: &str) -> Result<ParsedCallback, OAuthError> {
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| OAuthError::MalformedCallback("missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| OAuthError::MalformedCallback("missing target".into()))?;

    if method != "GET" {
        return Err(OAuthError::MalformedCallback(format!(
            "unexpected method {method}"
        )));
    }

    let synthetic = format!("http://127.0.0.1{target}");
    let url = Url::parse(&synthetic)
        .map_err(|e| OAuthError::MalformedCallback(format!("bad target: {e}")))?;

    if url.path() != CALLBACK_PATH {
        return Err(OAuthError::MalformedCallback(format!(
            "unexpected path {}",
            url.path()
        )));
    }

    let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
    Ok(ParsedCallback {
        code: params.get("code").cloned(),
        state: params.get("state").cloned(),
        error: params.get("error").cloned(),
    })
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn success_response() -> String {
    let body = "<!doctype html><meta charset=\"utf-8\"><title>slack-cli</title>\
        <body style=\"font-family:system-ui;padding:2em\">\
        <h2>Authentication complete.</h2>\
        <p>You can close this tab and return to the terminal.</p></body>";
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn error_response(reason: &str) -> String {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><body><h2>Authentication failed.</h2><p>{reason}</p></body>"
    );
    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_callback_with_code_and_state() {
        let line = "GET /callback?code=abc&state=xyz HTTP/1.1";
        let p = parse_request_line(line).unwrap();
        assert_eq!(p.code.as_deref(), Some("abc"));
        assert_eq!(p.state.as_deref(), Some("xyz"));
        assert!(p.error.is_none());
    }

    #[test]
    fn parses_error_callback() {
        let line = "GET /callback?error=access_denied HTTP/1.1";
        let p = parse_request_line(line).unwrap();
        assert_eq!(p.error.as_deref(), Some("access_denied"));
        assert!(p.code.is_none());
    }

    #[test]
    fn rejects_non_callback_path() {
        let line = "GET /other?code=abc HTTP/1.1";
        let err = parse_request_line(line).unwrap_err();
        assert!(matches!(err, OAuthError::MalformedCallback(_)));
    }

    #[test]
    fn rejects_non_get_method() {
        let line = "POST /callback HTTP/1.1";
        let err = parse_request_line(line).unwrap_err();
        assert!(matches!(err, OAuthError::MalformedCallback(_)));
    }
}
