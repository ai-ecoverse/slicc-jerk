use crate::browser::Browser;
use crate::exit::Exit;
use crate::oauth::{OAuthError, Token};
use jerk_oauth::capture;
use jerk_oauth::config::{CaptureFrom, Flow, OAuthConfig};
use jerk_oauth::exchange;
use jerk_oauth::expiry;
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

pub fn run(
    config: &OAuthConfig,
    scope: Option<&str>,
    browser: &dyn Browser,
    now_secs: u64,
    random_32: &[u8; 32],
    random_16: &[u8; 16],
    http_post: &dyn Fn(&str, &str) -> Result<String, String>,
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

    let mut vars: Vec<(&str, &str)> = vec![
        ("client_id", &config.client_id),
        ("redirect_uri", redirect_uri),
        ("scope", actual_scope),
        ("state", &state),
        ("nonce", &nonce),
    ];
    if let Some(c) = &challenge {
        vars.push(("code_challenge", c));
    }
    let authorize_url = template::render(&config.authorize_url, &vars);

    browser.open_url(&authorize_url).map_err(|e| OAuthError {
        exit: Exit::Unavailable,
        message: e,
    })?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let captured = wait_for_callback(&listener, &config.capture.from, deadline, bound_port, host)?;

    let result = capture::parse_redirect(
        &captured,
        &config.capture.from,
        &config.capture.param,
        Some(&state),
        Some(redirect_uri),
        actual_port,
    )
    .map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e.to_string(),
    })?;

    if config.relay.is_some() {
        if let Some(received_nonce) = result.params.get("nonce") {
            let expected = relay::nonce_from_bytes(random_16);
            relay::verify_nonce(&expected, received_nonce).map_err(|e| OAuthError {
                exit: Exit::Error,
                message: e.to_string(),
            })?;
        }
    }

    if config.flow == Flow::Pkce {
        let verifier = verifier.as_deref().unwrap();
        let token_url = config.token_url.as_deref().ok_or_else(|| OAuthError {
            exit: Exit::Usage,
            message: "token_url required for pkce flow".into(),
        })?;
        let body = exchange::build_authorization_code_body(
            &result.value,
            &config.client_id,
            redirect_uri,
            verifier,
        );
        let form_body = exchange::encode_form_body(&body);
        let response = http_post(token_url, &form_body).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e,
        })?;
        let token_response = exchange::parse_token_response(&response).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e.to_string(),
        })?;
        let expires_at = expiry::compute_expiry(now_secs, token_response.expires_in, None);
        Ok(Token {
            access_token: token_response.access_token.clone(),
            refresh_token: token_response.refresh_token.clone(),
            expires_at,
            scope: token_response.scope.clone(),
            token_type: token_response.token_type.clone(),
        })
    } else {
        let expires_in = result
            .params
            .get(config.expires_param.as_deref().unwrap_or("expires_in"))
            .and_then(|v| v.parse::<u64>().ok());
        let expires_at = expiry::compute_expiry(now_secs, expires_in, None);
        Ok(Token {
            access_token: result.value,
            refresh_token: None,
            expires_at,
            scope: result.params.get("scope").cloned(),
            token_type: result.params.get("token_type").cloned(),
        })
    }
}

const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

fn wait_for_callback(
    listener: &TcpListener,
    capture_from: &CaptureFrom,
    deadline: std::time::Instant,
    port: u16,
    host: &str,
) -> Result<String, OAuthError> {
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(OAuthError {
                exit: Exit::Error,
                message: "timed out waiting for callback".into(),
            });
        }

        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(_) => {
                std::thread::sleep(POLL_INTERVAL);
                continue;
            }
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

        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(_) => {
                std::thread::sleep(POLL_INTERVAL);
                continue;
            }
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
    let mut body = vec![0u8; length.min(8192)];
    reader.read_exact(&mut body).ok()?;
    Some(String::from_utf8_lossy(&body).to_string())
}

pub fn try_bind(port: u16) -> bool {
    TcpListener::bind(format!("127.0.0.1:{port}")).is_ok()
}
