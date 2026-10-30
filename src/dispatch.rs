// Shared JSON-RPC / MCP dispatch shared by both transports (stdio and HTTP).
use json::JsonValue;

pub enum Outcome {
    /// Send this JSON-RPC response.
    Response(JsonValue),
    /// Accepted notification; no response body (HTTP: 202, stdio: silence).
    Notification,
    /// Malformed JSON at the framing level (stdio: -32700, HTTP: 400).
    BadRequest,
}

/// HTTP-level outcome: supports JSON-RPC batch arrays (2025-03-26 clients).
pub enum HttpOutcome {
    /// At least one response to send (single object, or array for batches).
    Responses(Vec<JsonValue>),
    /// All messages were notifications; nothing to send.
    Accepted,
    /// Framing-level failure (bad JSON, empty/oversized batch).
    BadRequest,
}

const MAX_BATCH: usize = 16;

fn build_resp(id: JsonValue, result: Option<JsonValue>, err: Option<(i64, &str)>) -> JsonValue {
    let mut resp = json::object::Object::new();
    resp.insert("jsonrpc", "2.0".into());
    resp.insert("id", id);
    if let Some(r) = result {
        resp.insert("result", r);
    }
    if let Some((code, msg)) = err {
        let mut e = json::object::Object::new();
        e.insert("code", JsonValue::from(code as f64));
        e.insert("message", msg.into());
        resp.insert("error", JsonValue::Object(e));
    }
    JsonValue::Object(resp)
}

fn process_msg(req: &JsonValue) -> Outcome {
    let id = req["id"].clone();
    if id.is_null() {
        return Outcome::Notification;
    }
    let method = req["method"].as_str().unwrap_or("");
    match method {
        "initialize" => Outcome::Response(build_resp(id, Some(super::tools::init_result(&req["params"])), None)),
        "ping" => Outcome::Response(build_resp(
            id,
            Some(JsonValue::Object(json::object::Object::new())),
            None,
        )),
        "tools/list" => Outcome::Response(build_resp(id, Some(super::tools::tool_defs()), None)),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            if name.is_empty() {
                Outcome::Response(build_resp(id, None, Some((-32602, "tools/call requires a tool name"))))
            } else {
                let args = req["params"]["arguments"].clone();
                Outcome::Response(build_resp(id, Some(super::tools::call_tool(name, &args)), None))
            }
        }
        _ => Outcome::Response(build_resp(id, None, Some((-32601, "method not found")))),
    }
}

/// Process one newline-delimited JSON-RPC message (stdio mode).
pub fn process(body: &str) -> Outcome {
    match json::parse(body) {
        Ok(v) => process_msg(&v),
        Err(_) => Outcome::BadRequest,
    }
}

/// Process one HTTP POST body: single message or a JSON-RPC batch array.
pub fn process_http(body: &str) -> HttpOutcome {
    let Ok(v) = json::parse(body) else {
        return HttpOutcome::BadRequest;
    };
    if let JsonValue::Array(items) = &v {
        if items.is_empty() || items.len() > MAX_BATCH {
            return HttpOutcome::BadRequest;
        }
        let mut responses: Vec<JsonValue> = Vec::new();
        for item in items {
            match process_msg(item) {
                Outcome::Response(r) => responses.push(r),
                Outcome::Notification => {}
                Outcome::BadRequest => return HttpOutcome::BadRequest,
            }
        }
        if responses.is_empty() {
            return HttpOutcome::Accepted;
        }
        return HttpOutcome::Responses(responses);
    }
    match process_msg(&v) {
        Outcome::Response(r) => HttpOutcome::Responses(vec![r]),
        Outcome::Notification => HttpOutcome::Accepted,
        Outcome::BadRequest => HttpOutcome::BadRequest,
    }
}
