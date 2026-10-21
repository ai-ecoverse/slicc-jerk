use crate::browser::Browser;
use crate::exit::Exit;
use crate::oauth::{self, OAuthError, Token};
use jerk_oauth::capture;
use jerk_oauth::config::{CaptureFrom, Flow, OAuthConfig};
use jerk_oauth::pkce;
use jerk_oauth::relay;
use jerk_oauth::template;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

const CALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html><head><title>Signed in</title></head><body>
<p id="msg">Signed in, you can close this tab.</p>
<script>
if (location.hash) {
  var x = new XMLHttpRequest();
  x.open("POST", "/hash");
  x.send(location.hash.substring(1));
}
</script>
</body></html>
"#;

const ACK_HTML: &str = r#"<!DOCTYPE html>
<html><head><title>Signed in</title></head><body>
<p>Signed in, you can close this tab.</p>
<script>window.close()</script>
</body></html>
"#;

#[allow(clippy::too_many_arguments)]
pub fn run(
    config: &OAuthConfig,
    scope: Option<&str>,
    browser: &dyn Browser,
    now_secs: u64,
    random_32: &[u8; 32],
    random_16: &[u8; 16],
    http_post: &oauth::HttpPost<'_>,
    extra_vars: &[(&str, &str)],
    cors_origins: &[String],
) -> Result<Token, OAuthError> {
    let redirect_uri = &config.redirect_uri;
    let parsed = url::Url::parse(redirect_uri).map_err(|e| OAuthError {
        exit: Exit::Usage,
        message: format!("invalid redirect_uri: {e}"),
    })?;
    let port: u16 = parsed.port().ok_or_else(|| OAuthError {
        exit: Exit::Usage,
        message: "redirect_uri must include a port".into(),
    })?;
    let host = parsed.host_str().unwrap_or("127.0.0.1");
    let redirect_path = parsed.path().to_string();

    let bind_addr = match host {
        "127.0.0.1" | "localhost" => format!("127.0.0.1:{port}"),
        "[::1]" => format!("[::1]:{port}"),
        _ => {
            return Err(OAuthError {
                exit: Exit::Usage,
                message: "loopback redirect_uri must use 127.0.0.1, localhost or [::1]".into(),
            })
        }
    };

    let listener = TcpListener::bind(&bind_addr).map_err(|e| OAuthError {
        exit: Exit::Unavailable,
        message: format!("cannot bind {bind_addr}: {e}"),
    })?;

    let actual_port = listener.local_addr().ok().map(|a| a.port());

    let timeout_ms = jerk_oauth::config::timeout_ms(config);

    let _ = listener.set_nonblocking(true);

    let (verifier, challenge) = if config.flow == Flow::Pkce {
        let v = pkce::verifier(random_32);
        let c = pkce::challenge(&v);
        (Some(v), Some(c))
    } else {
        (None, None)
    };

    let state = if let Some(r) = &config.relay {
        relay::build_state(r, random_16)
    } else {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        URL_SAFE_NO_PAD.encode(random_16)
    };

    let nonce = {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        URL_SAFE_NO_PAD.encode(&random_16[..8])
    };

    let actual_scope = scope.or(config.scope.as_deref()).unwrap_or("");

    for name in template::extra_placeholders(&config.authorize_url) {
        let value = extra_vars
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| *v)
            .unwrap_or("");
        if value.is_empty() {
            return Err(OAuthError {
                exit: Exit::Usage,
                message: format!("pass -p {name}=… (required for this provider's login URL)"),
            });
        }
    }

    let mut vars: Vec<(&str, &str)> = vec![
        ("client_id", &config.client_id),
        ("redirect_uri", redirect_uri),
        ("scope", actual_scope),
        ("state", &state),
        ("nonce", &nonce),
    ];
    vars.extend_from_slice(extra_vars);
    if let Some(c) = &challenge {
        vars.push(("code_challenge", c));
    }
    let authorize_url = template::render(&config.authorize_url, &vars);
    if authorize_url.contains('{') {
        return Err(OAuthError {
            exit: Exit::Usage,
            message: format!(
                "authorize_url still has placeholders after substitution: {authorize_url}"
            ),
        });
    }

    browser.open_url(&authorize_url).map_err(|e| OAuthError {
        exit: Exit::Unavailable,
        message: e,
    })?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let captured = wait_for_callback(
        &listener,
        &config.capture.from,
        &redirect_path,
        deadline,
        bound_port,
        host,
        cors_origins,
    )?;

    let expected_state = if jerk_oauth::config::expects_state(&config.authorize_url) {
        Some(state.as_str())
    } else {
        None
    };
    let result = if config.capture.from == CaptureFrom::Body {
        capture::parse_json_body(&captured, &config.capture.param, expected_state).map_err(|e| {
            OAuthError {
                exit: Exit::Error,
                message: e.to_string(),
            }
        })?
    } else {
        capture::parse_redirect(
            &captured,
            &config.capture.from,
            &config.capture.param,
            expected_state,
            Some(redirect_uri),
            actual_port,
        )
        .map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e.to_string(),
        })?
    };

    if config.relay.is_some() {
        if let Some(received_nonce) = result.params.get("nonce") {
            let expected = relay::nonce_from_bytes(random_16);
            relay::verify_nonce(&expected, received_nonce).map_err(|e| OAuthError {
                exit: Exit::Error,
                message: e.to_string(),
            })?;
        }
    }

    oauth::token_from_capture(
        config,
        &result,
        verifier.as_deref(),
        scope,
        now_secs,
        http_post,
    )
}

const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
const BODY_ACK_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

fn wait_for_callback(
    listener: &TcpListener,
    capture_from: &CaptureFrom,
    redirect_path: &str,
    deadline: std::time::Instant,
    port: u16,
    host: &str,
    cors_origins: &[String],
) -> Result<String, OAuthError> {
    if *capture_from == CaptureFrom::Body {
        return wait_for_json_body(listener, redirect_path, deadline, cors_origins);
    }

    loop {
        if std::time::Instant::now() >= deadline {
            return Err(OAuthError {
                exit: Exit::Error,
                message: "timed out waiting for callback".into(),
            });
        }

        let Some(stream) = oauth::accept_blocking(listener) else {
            std::thread::sleep(POLL_INTERVAL);
            continue;
        };

        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 || parts[0] != "GET" {
            continue;
        }

        let full_url = format!("http://{host}:{port}{}", parts[1]);

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            CALLBACK_HTML.len(),
            CALLBACK_HTML
        );
        let _ = (&stream).write_all(response.as_bytes());
        drop(stream);

        if *capture_from == CaptureFrom::Fragment {
            return wait_for_fragment(listener, &full_url, deadline, port, host);
        }

        return Ok(full_url);
    }
}

fn request_path(target: &str) -> &str {
    let without_query = target.split('?').next().unwrap_or(target);
    if let Some(rest) = without_query.strip_prefix("http://") {
        return rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
    }
    if let Some(rest) = without_query.strip_prefix("https://") {
        return rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
    }
    without_query
}

fn wait_for_json_body(
    listener: &TcpListener,
    redirect_path: &str,
    deadline: std::time::Instant,
    cors_origins: &[String],
) -> Result<String, OAuthError> {
    let mut captured: Option<String> = None;
    let mut ack_deadline: Option<std::time::Instant> = None;
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            if let Some(body) = captured {
                return Ok(body);
            }
            return Err(OAuthError {
                exit: Exit::Error,
                message: "timed out waiting for callback".into(),
            });
        }
        if let (Some(body), Some(until)) = (&captured, ack_deadline) {
            if now >= until {
                return Ok(body.clone());
            }
        }

        let Some(stream) = oauth::accept_blocking(listener) else {
            std::thread::sleep(POLL_INTERVAL);
            continue;
        };

        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let method = parts[0];
        let path = request_path(parts[1]);
        let path_ok = path == redirect_path;

        let mut line = String::new();
        let mut content_length: usize = 0;
        let mut origin = String::new();
        let mut private_network = false;
        loop {
            line.clear();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(val) = lower.strip_prefix("content-length:") {
                content_length = val.trim().parse().unwrap_or(0);
            }
            if let Some(val) = lower.strip_prefix("origin:") {
                origin = val.trim().to_string();
            }
            if lower.starts_with("access-control-request-private-network:")
                && lower.contains("true")
            {
                private_network = true;
            }
        }

        let cors = cors_headers(cors_origins, &origin, private_network);

        if method == "OPTIONS" && path_ok {
            let response = format!(
                "HTTP/1.1 200 OK\r\n{cors}Access-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: content-type\r\nCache-Control: no-store, private, must-revalidate\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = (&stream).write_all(response.as_bytes());
            continue;
        }

        if method == "POST" && path_ok {
            let body = match read_body(&mut reader, content_length) {
                Some(b) if !b.is_empty() => b,
                _ => {
                    let msg = "Missing body";
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\n{cors}Content-Type: text/plain\r\nCache-Control: no-store, private, must-revalidate\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{msg}",
                        msg.len()
                    );
                    let _ = (&stream).write_all(response.as_bytes());
                    continue;
                }
            };
            let msg = "Login successful.";
            let response = format!(
                "HTTP/1.1 200 OK\r\n{cors}Content-Type: text/plain\r\nCache-Control: no-store, private, must-revalidate\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{msg}",
                msg.len()
            );
            let _ = (&stream).write_all(response.as_bytes());
            captured = Some(body);
            ack_deadline = Some(std::time::Instant::now() + BODY_ACK_GRACE);
            continue;
        }

        if method == "GET" && path_ok {
            let response = format!(
                "HTTP/1.1 200 OK\r\n{cors}Content-Type: text/html\r\nCache-Control: no-store, private, must-revalidate\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                ACK_HTML.len(),
                ACK_HTML
            );
            let _ = (&stream).write_all(response.as_bytes());
            if let Some(body) = captured {
                return Ok(body);
            }
            continue;
        }

        let msg = "Not Found";
        let response = format!(
            "HTTP/1.1 404 Not Found\r\n{cors}Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{msg}",
            msg.len()
        );
        let _ = (&stream).write_all(response.as_bytes());
    }
}

fn cors_headers(allowed: &[String], origin: &str, private_network: bool) -> String {
    let mut out = String::new();
    if allowed.iter().any(|o| o == origin) {
        out.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
    }
    if private_network {
        out.push_str("Access-Control-Allow-Private-Network: true\r\n");
    }
    out
}

fn wait_for_fragment(
    listener: &TcpListener,
    base_url: &str,
    deadline: std::time::Instant,
    _port: u16,
    _host: &str,
) -> Result<String, OAuthError> {
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(OAuthError {
                exit: Exit::Error,
                message: "timed out waiting for fragment data".into(),
            });
        }

        let Some(stream) = oauth::accept_blocking(listener) else {
            std::thread::sleep(POLL_INTERVAL);
            continue;
        };

        if let Some(frag) = try_read_fragment(&stream) {
            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = (&stream).write_all(resp.as_bytes());
            return Ok(format!("{base_url}#{frag}"));
        }
    }
}

pub fn try_read_fragment(stream: &std::net::TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "POST" || parts[1] != "/hash" {
        return None;
    }

    let content_length = read_content_length(&mut reader);
    if content_length == 0 {
        return None;
    }

    read_body(&mut reader, content_length)
}

fn read_content_length(reader: &mut BufReader<&std::net::TcpStream>) -> usize {
    let mut content_length: usize = 0;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }
    content_length
}

fn read_body(reader: &mut BufReader<&std::net::TcpStream>, length: usize) -> Option<String> {
    let mut body = vec![0u8; length.min(65536)];
    reader.read_exact(&mut body).ok()?;
    Some(String::from_utf8_lossy(&body).to_string())
}

pub fn try_bind(port: u16) -> bool {
    TcpListener::bind(format!("127.0.0.1:{port}")).is_ok()
}
