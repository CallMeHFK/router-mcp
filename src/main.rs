// router-mcp (Rust): minimal MCP server for ImmortalWrt/OpenWrt routers.
//
// Two transports, one binary:
//   router-mcp                     stdio mode (newline-delimited JSON-RPC 2.0,
//                                  MCP stdio spec; used behind SSH)
//   router-mcp http [<listen>]     Streamable HTTP mode (MCP 2025-06-18);
//                                  <listen> or ROUTER_MCP_LISTEN, default
//                                  127.0.0.1:8420. Non-loopback listeners
//                                  require a Bearer token from
//                                  ROUTER_MCP_TOKEN / ROUTER_MCP_TOKEN_FILE.
//
// Security invariants (both modes):
//   - every std::process::Command call site uses literal arguments only;
//     request data is used exclusively as match keys, never interpolated
//     into a command line; no shell is ever spawned
//   - unknown tools and methods never reach process spawning
//   - no listening sockets in stdio mode; HTTP mode is fail-closed on tokens
//
// Dependency note: the `json` crate is used instead of serde because it has
// no build scripts and no proc-macros, so the whole build only runs the
// signed Rust toolchain (compatible with Windows Smart App Control).
mod dispatch;
mod http;
mod tools;

use json::JsonValue;
use std::io::{BufRead, BufWriter, Write};

fn run_stdio() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match dispatch::process(line) {
            dispatch::Outcome::Response(v) => {
                let _ = writeln!(out, "{}", v);
                let _ = out.flush();
            }
            dispatch::Outcome::Notification => {} // no response for notifications
            dispatch::Outcome::BadRequest => {
                // JSON-RPC parse errors answer with id null.
                let mut resp = json::object::Object::new();
                resp.insert("jsonrpc", "2.0".into());
                resp.insert("id", JsonValue::Null);
                let mut e = json::object::Object::new();
                e.insert("code", JsonValue::from(-32700f64));
                e.insert("message", "parse error".into());
                resp.insert("error", JsonValue::Object(e));
                let _ = writeln!(out, "{}", JsonValue::Object(resp));
                let _ = out.flush();
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "http" || args[1] == "--http") {
        let listen = args
            .get(2)
            .cloned()
            .or_else(|| std::env::var("ROUTER_MCP_LISTEN").ok())
            .unwrap_or_else(|| "127.0.0.1:8420".to_string());
        http::serve(&listen);
        return;
    }
    run_stdio();
}
