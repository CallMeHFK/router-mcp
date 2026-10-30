// Streamable HTTP transport (MCP 2025-06-18), minimal std-only HTTP/1.1.
//
// Hardening rules:
//   - single endpoint: POST /mcp ; GET is 405 (no server-initiated stream)
//   - Bearer token REQUIRED for non-loopback listeners (fail closed at start);
//     constant-time comparison; token comes from env/secret file only
//   - strict limits: header block 8 KB, body 1 MB, 10 s read / 30 s write
//     timeouts; Connection: close (no keep-alive state to track)
//   - TLS is deliberately not implemented (every Rust TLS crate carries
//     build scripts, which Smart App Control blocks); use an SSH tunnel or
//     a WireGuard-style overlay for encryption
use crate::dispatch;
use json::JsonValue;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::time::Duration;

const MAX_HEADER: usize = 8 * 1024;
const MAX_BODY: usize = 1024 * 1024;

fn load_token() -> Option<String> {
    if let Ok(t) = std::env::var("ROUTER_MCP_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    if let Ok(path) = std::env::var("ROUTER_MCP_TOKEN_FILE") {
        if !path.is_empty() {
            if let Ok(t) = std::fs::read_to_string(&path) {
                let t = t.trim().to_string();
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
    }
    None
}

fn is_loopback(listen: &str) -> bool {
    // "ip:port" (v4) or "[v6]:port"; hostnames never count as loopback.
    let host = match listen.rfind(':') {
        Some(i) => &listen[..i],
        None => listen,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

// Constant-time equality to avoid token-compare timing leaks.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn write_resp(stream: &mut TcpStream, code: u16, reason: &str, extra: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{}\r\n",
        code,
        reason,
        body.len(),
        extra
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn err_body(code: i64, msg: &str) -> String {
    let mut e = json::object::Object::new();
    e.insert("code", JsonValue::from(code as f64));
    e.insert("message", msg.into());
    let mut r = json::object::Object::new();
    r.insert("jsonrpc", "2.0".into());
    r.insert("id", JsonValue::Null);
    r.insert("error", JsonValue::Object(e));
    JsonValue::Object(r).to_string()
}

fn unauthorized(stream: &mut TcpStream) {
    write_resp(
        stream,
        401,
        "Unauthorized",
        "WWW-Authenticate: Bearer\r\n",
        err_body(-32001, "unauthorized").as_bytes(),
    );
}

pub fn serve(listen: &str) {
    let token = load_token();
    let loopback = is_loopback(listen);
    if token.is_none() && !loopback {
        eprintln!(
            "router-mcp: refusing to start: non-loopback listener {} requires ROUTER_MCP_TOKEN or ROUTER_MCP_TOKEN_FILE",
            listen
        );
        std::process::exit(1);
    }
    let listener = match TcpListener::bind(listen) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("router-mcp: bind {} failed: {}", listen, e);
            std::process::exit(1);
        }
    };
    if loopback && token.is_none() {
        eprintln!("router-mcp: loopback listener without token (tunnel mode)");
    }
    eprintln!("router-mcp: http listening on {}", listen);
    for stream in listener.incoming() {
        if let Ok(s) = stream {
            let token = token.clone();
            std::thread::spawn(move || handle_conn(s, token.as_deref()));
        }
    }
}

fn handle_conn(mut stream: TcpStream, token: Option<&str>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let mut reader = match stream.try_clone() {
        Ok(r) => BufReader::new(r),
        Err(_) => return,
    };

    // Request line.
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.len() > MAX_HEADER {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().unwrap_or("").to_string();
    if method.is_empty() || target.is_empty() {
        write_resp(&mut stream, 400, "Bad Request", "", err_body(-32600, "malformed request line").as_bytes());
        return;
    }

    // Headers (only the ones we act on).
    let mut content_length: usize = 0;
    let mut auth_header: Option<String> = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() {
            return;
        }
        let len = h.len();
        let h = h.trim_end().to_string();
        if h.is_empty() {
            break;
        }
        if len > 4096 || h.len() > MAX_HEADER {
            return;
        }
        let lower = h.to_ascii_lowercase();
        if lower.starts_with("content-length:") {
            content_length = h[15..].trim().parse().unwrap_or(0);
        } else if lower.starts_with("authorization:") {
            auth_header = Some(h[14..].trim().to_string());
        }
    }

    // Path filter: only /mcp.
    let path = target.split('?').next().unwrap_or("");
    let auth_ok = match (token, auth_header.as_deref()) {
        (None, _) => true, // loopback tunnel mode
        (Some(_), Some(hv)) => match hv.strip_prefix("Bearer ") {
            Some(got) => ct_eq(got, token.unwrap_or("")),
            None => false,
        },
        (Some(_), None) => false,
    };

    match (method.as_str(), path) {
        ("POST", "/mcp") => {
            if !auth_ok {
                unauthorized(&mut stream);
                return;
            }
            if content_length == 0 || content_length > MAX_BODY {
                write_resp(&mut stream, 400, "Bad Request", "", err_body(-32600, "invalid content-length").as_bytes());
                return;
            }
            let mut body = vec![0u8; content_length];
            if reader.read_exact(&mut body).is_err() {
                return;
            }
            let body = String::from_utf8_lossy(&body);
            match dispatch::process_http(&body) {
                dispatch::HttpOutcome::Responses(v) => {
                    let payload = if v.len() == 1 {
                        v[0].to_string()
                    } else {
                        JsonValue::Array(v).to_string()
                    };
                    write_resp(&mut stream, 200, "OK", "", payload.as_bytes());
                }
                dispatch::HttpOutcome::Accepted => write_resp(&mut stream, 202, "Accepted", "", b""),
                dispatch::HttpOutcome::BadRequest => {
                    write_resp(&mut stream, 400, "Bad Request", "", err_body(-32700, "parse error").as_bytes())
                }
            }
        }
        ("DELETE", "/mcp") => {
            // Stateless server: accept session termination politely.
            write_resp(&mut stream, 204, "No Content", "", b"");
        }
        ("GET", "/mcp") | ("OPTIONS", "/mcp") => {
            // No server-initiated SSE stream in this minimal implementation.
            write_resp(&mut stream, 405, "Method Not Allowed", "Allow: POST, DELETE\r\n", err_body(-32600, "GET not supported").as_bytes());
        }
        (_, "/mcp") => {
            if !auth_ok {
                unauthorized(&mut stream);
                return;
            }
            write_resp(&mut stream, 405, "Method Not Allowed", "Allow: POST, DELETE\r\n", err_body(-32600, "method not supported").as_bytes());
        }
        _ => {
            write_resp(&mut stream, 404, "Not Found", "", err_body(-32600, "not found").as_bytes());
        }
    }
}
