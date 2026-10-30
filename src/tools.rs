// Tool implementations. Security invariant: every Command below uses literal
// arguments only; validated request values select between fixed commands.
use json::JsonValue;
use std::process::Command;

// run_out executes a literal command and returns (trimmed combined output, success).
// Takes &mut Command because Command::args() chains yield &mut Command.
fn run_out(cmd: &mut Command) -> (String, bool) {
    match cmd.output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            (s.trim().to_string(), o.status.success())
        }
        Err(e) => (format!("exec failed: {}", e), false),
    }
}

fn text_result(text: String, is_err: bool) -> JsonValue {
    let mut item = json::object::Object::new();
    item.insert("type", "text".into());
    item.insert("text", JsonValue::String(text));
    let mut r = json::object::Object::new();
    r.insert("content", JsonValue::Array(vec![JsonValue::Object(item)]));
    if is_err {
        r.insert("isError", true.into());
    }
    JsonValue::Object(r)
}

fn tool_error(msg: impl Into<String>) -> JsonValue {
    text_result(msg.into(), true)
}

const TOOLS_JSON: &str = r#"{"tools": [
  {"name": "system_info", "description": "Model, firmware, uptime, load, memory, overlay usage, SoC temperature.", "inputSchema": {"type": "object"}},
  {"name": "network_status", "description": "WAN/WAN6/LAN interface status: up/down, addresses, gateway.", "inputSchema": {"type": "object"}},
  {"name": "connected_devices", "description": "DHCP leases, IPv6 neighbors, Wi-Fi associated stations.", "inputSchema": {"type": "object"}},
  {"name": "wifi_control", "description": "Enable/disable a Wi-Fi radio or read its state. WARNING: taking a radio down disconnects its clients immediately.", "inputSchema": {"type": "object", "properties": {"radio": {"type": "string", "enum": ["radio0", "radio1", "all"]}, "action": {"type": "string", "enum": ["up", "down", "status"]}}, "required": ["radio", "action"]}},
  {"name": "firewall_summary", "description": "Firewall zones, policies, rule and port-forward counts.", "inputSchema": {"type": "object"}},
  {"name": "recent_logs", "description": "Last N system log lines. Default 50, max 200.", "inputSchema": {"type": "object", "properties": {"lines": {"type": "integer", "minimum": 1, "maximum": 200}}}},
  {"name": "reboot", "description": "Reboot the router; requires confirm=true. The SSH connection drops.", "inputSchema": {"type": "object", "properties": {"confirm": {"type": "boolean"}}, "required": ["confirm"]}},
  {"name": "network_traffic", "description": "Per-interface RX/TX byte counters with a 1-second rate sample (bytes/s), plus conntrack entry count.", "inputSchema": {"type": "object"}},
  {"name": "package_list", "description": "All installed opkg packages (count plus name - version lines).", "inputSchema": {"type": "object"}},
  {"name": "port_forwards", "description": "Manage WAN port-forward (DNAT) rules. action=list|add|remove. add/remove require confirm=true and reload the firewall; dest_ip must be a private IPv4.", "inputSchema": {"type": "object", "properties": {"action": {"type": "string", "enum": ["list", "add", "remove"]}, "name": {"type": "string", "pattern": "^[A-Za-z0-9_-]{1,32}$"}, "proto": {"type": "string", "enum": ["tcp", "udp", "tcpudp"]}, "src_dport": {"type": "integer", "minimum": 1, "maximum": 65535}, "dest_ip": {"type": "string"}, "dest_port": {"type": "integer", "minimum": 1, "maximum": 65535}, "confirm": {"type": "boolean"}}, "required": ["action"]}}
]}"#;

pub fn tool_defs() -> JsonValue {
    json::parse(TOOLS_JSON).expect("static tools json is valid")
}

pub fn call_tool(name: &str, args: &JsonValue) -> JsonValue {
    match name {
        "system_info" => system_info(),
        "network_status" => network_status(),
        "connected_devices" => connected_devices(),
        "wifi_control" => wifi_control(args),
        "firewall_summary" => firewall_summary(),
        "recent_logs" => recent_logs(args),
        "reboot" => reboot(args),
        "network_traffic" => network_traffic(),
        "package_list" => package_list(),
        "port_forwards" => port_forwards(args),
        _ => tool_error("unknown tool"),
    }
}

fn system_info() -> JsonValue {
    let mut out = json::object::Object::new();
    let (v, ok) = run_out(Command::new("ubus").args(["call", "system", "board"]));
    if ok {
        out.insert("board", v.into());
    }
    let (v, ok) = run_out(Command::new("ubus").args(["call", "system", "info"]));
    if ok {
        out.insert("system", v.into());
    }
    let (v, ok) = run_out(Command::new("df").args(["-k", "/overlay"]));
    if ok {
        out.insert("overlay_df", v.into());
    }
    if let Ok(s) = std::fs::read_to_string("/proc/loadavg") {
        out.insert("loadavg", s.trim().into());
    }
    let mut temps: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/thermal") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("thermal_zone") {
                if let Ok(s) = std::fs::read_to_string(format!("/sys/class/thermal/{}/temp", name)) {
                    temps.push(s.trim().to_string());
                }
            }
        }
    }
    temps.sort();
    out.insert("temps_millicelsius", temps.join(",").into());
    text_result(JsonValue::Object(out).to_string(), false)
}

fn network_status() -> JsonValue {
    let mut out = json::object::Object::new();
    for name in ["wan", "wan6", "lan"] {
        // Loop variable over literals; each arm is a fixed invocation.
        let (v, ok) = match name {
            "wan" => run_out(Command::new("ifstatus").arg("wan")),
            "wan6" => run_out(Command::new("ifstatus").arg("wan6")),
            _ => run_out(Command::new("ifstatus").arg("lan")),
        };
        let v = if ok { v } else { "ifstatus failed".to_string() };
        out.insert(name, v.into());
    }
    text_result(JsonValue::Object(out).to_string(), false)
}

fn connected_devices() -> JsonValue {
    let mut out = json::object::Object::new();

    let mut leases: Vec<JsonValue> = Vec::new();
    if let Ok(s) = std::fs::read_to_string("/tmp/dhcp.leases") {
        for line in s.trim().lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 4 {
                let mut m = json::object::Object::new();
                m.insert("expires_epoch", f[0].into());
                m.insert("mac", f[1].into());
                m.insert("ip", f[2].into());
                m.insert("hostname", (if f[3] == "*" { "" } else { f[3] }).into());
                leases.push(JsonValue::Object(m));
            }
        }
    }
    out.insert("dhcp_leases", JsonValue::Array(leases));

    let (v, ok) = run_out(Command::new("ip").args(["neigh", "show"]));
    if ok {
        out.insert("ipv6_neigh", v.into());
    }

    let mut stations = json::object::Object::new();
    // This router names its radios wl0-ap0 / wl1-ap0 (not wlan0/wlan1).
    let (v, ok) = run_out(Command::new("iwinfo").args(["wl0-ap0", "assoclist"]));
    if ok && !v.is_empty() {
        stations.insert("wl0-ap0", v.into());
    }
    let (v, ok) = run_out(Command::new("iwinfo").args(["wl1-ap0", "assoclist"]));
    if ok && !v.is_empty() {
        stations.insert("wl1-ap0", v.into());
    }
    out.insert("wifi_stations", JsonValue::Object(stations));
    text_result(JsonValue::Object(out).to_string(), false)
}

fn wifi_control(args: &JsonValue) -> JsonValue {
    let Some(radio) = args["radio"].as_str() else {
        return tool_error("radio must be radio0, radio1 or all");
    };
    let Some(action) = args["action"].as_str() else {
        return tool_error("action must be up, down or status");
    };
    if !matches!(radio, "radio0" | "radio1" | "all") {
        return tool_error("radio must be radio0, radio1 or all");
    }
    if !matches!(action, "up" | "down" | "status") {
        return tool_error("action must be up, down or status");
    }

    if action == "status" {
        // uci get fails when the option is unset, which means enabled.
        let mut states = json::object::Object::new();
        if radio == "radio0" || radio == "all" {
            let (v, _) = run_out(Command::new("uci").args(["-q", "get", "wireless.radio0.disabled"]));
            states.insert("radio0", (if v == "1" { "disabled" } else { "enabled" }).into());
        }
        if radio == "radio1" || radio == "all" {
            let (v, _) = run_out(Command::new("uci").args(["-q", "get", "wireless.radio1.disabled"]));
            states.insert("radio1", (if v == "1" { "disabled" } else { "enabled" }).into());
        }
        return text_result(JsonValue::Object(states).to_string(), false);
    }

    // up/down: each (radio, action) pair maps to one literal command.
    if action == "up" {
        if radio == "radio0" || radio == "all" {
            let (_, ok) = run_out(Command::new("uci").args(["set", "wireless.radio0.disabled=0"]));
            if !ok {
                return tool_error("uci set radio0 failed");
            }
        }
        if radio == "radio1" || radio == "all" {
            let (_, ok) = run_out(Command::new("uci").args(["set", "wireless.radio1.disabled=0"]));
            if !ok {
                return tool_error("uci set radio1 failed");
            }
        }
    } else {
        if radio == "radio0" || radio == "all" {
            let (_, ok) = run_out(Command::new("uci").args(["set", "wireless.radio0.disabled=1"]));
            if !ok {
                return tool_error("uci set radio0 failed");
            }
        }
        if radio == "radio1" || radio == "all" {
            let (_, ok) = run_out(Command::new("uci").args(["set", "wireless.radio1.disabled=1"]));
            if !ok {
                return tool_error("uci set radio1 failed");
            }
        }
    }

    let (_, ok) = run_out(Command::new("uci").args(["commit", "wireless"]));
    if !ok {
        return tool_error("uci commit failed");
    }
    let (_, ok) = run_out(Command::new("wifi").args(["reload"]));
    if !ok {
        return tool_error("wifi reload failed");
    }

    let mut states = json::object::Object::new();
    if radio == "radio0" || radio == "all" {
        states.insert("radio0", action.into());
    }
    if radio == "radio1" || radio == "all" {
        states.insert("radio1", action.into());
    }
    let mut res = json::object::Object::new();
    res.insert("applied", action.into());
    res.insert("states", JsonValue::Object(states));
    text_result(JsonValue::Object(res).to_string(), false)
}

fn firewall_summary() -> JsonValue {
    let (raw, ok) = run_out(Command::new("uci").args(["-q", "show", "firewall"]));
    if !ok && raw.is_empty() {
        return tool_error("uci show firewall failed");
    }

    #[derive(Default)]
    struct Zone {
        name: String,
        input: String,
        output: String,
        forward: String,
    }
    // BTreeMap keeps zones ordered by section index.
    let mut zones: std::collections::BTreeMap<i64, Zone> = std::collections::BTreeMap::new();
    let mut rule_total = 0i64;
    let mut rule_disabled = 0i64;
    let mut redirect_total = 0i64;
    let mut redirects: Vec<JsonValue> = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim_matches('\'');

        if let Some(rest) = k.strip_prefix("firewall.@zone[") {
            if let Some(end) = rest.find(']') {
                if let Ok(n) = rest[..end].parse::<i64>() {
                    let z = zones.entry(n).or_default();
                    if k.ends_with(".name") {
                        z.name = v.to_string();
                    } else if k.ends_with(".input") {
                        z.input = v.to_string();
                    } else if k.ends_with(".output") {
                        z.output = v.to_string();
                    } else if k.ends_with(".forward") {
                        z.forward = v.to_string();
                    }
                }
            }
        } else if k.starts_with("firewall.@rule[") {
            rule_total += 1;
            if k.ends_with(".enabled") && v == "0" {
                rule_disabled += 1;
            }
        } else if k.starts_with("firewall.@redirect[") {
            if k.ends_with(".name") {
                redirects.push(v.into());
            }
            // Section head line: firewall.@redirect[N]=redirect
            if k.ends_with(']') && v == "redirect" {
                redirect_total += 1;
            }
        }
    }

    let zone_list: Vec<JsonValue> = zones
        .into_values()
        .map(|z| {
            let mut m = json::object::Object::new();
            m.insert("name", z.name.into());
            m.insert("input", z.input.into());
            m.insert("output", z.output.into());
            m.insert("forward", z.forward.into());
            JsonValue::Object(m)
        })
        .collect();

    let mut res = json::object::Object::new();
    res.insert("zones", JsonValue::Array(zone_list));
    res.insert("rules_total", JsonValue::from(rule_total as f64));
    res.insert("rules_disabled", JsonValue::from(rule_disabled as f64));
    res.insert("port_forwards", JsonValue::Array(redirects));
    res.insert("port_forward_count", JsonValue::from(redirect_total as f64));
    // Full redirect details come from the config file itself.
    if let Ok(fw) = std::fs::read_to_string(FW_PATH) {
        res.insert("redirect_details", JsonValue::Array(redirect_list_value(&fw)));
    }
    text_result(JsonValue::Object(res).to_string(), false)
}

// ---------------------------------------------- firewall config (redirects)
//
// Port-forward management edits /etc/config/firewall directly (atomic
// tmp+rename, pre-write backup) so that no request data ever reaches an
// argv; the only exec afterwards is the literal `fw4 reload`. All user
// values are strictly validated (name charset, proto enum, u16 ports,
// private IPv4) before they may enter the file content.

const FW_PATH: &str = "/etc/config/firewall";
const FW_BACKUP: &str = "/etc/router-mcp-firewall.bak";

struct RedirectBlock {
    start: usize,
    end: usize,
    fields: json::object::Object,
}

fn scan_redirects(raw: &str) -> Vec<RedirectBlock> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut blocks: Vec<RedirectBlock> = Vec::new();
    let mut cur: Option<(usize, json::object::Object)> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("config ") {
            if let Some((s, f)) = cur.take() {
                blocks.push(RedirectBlock { start: s, end: i, fields: f });
            }
            let rest = t["config ".len()..].trim();
            let typ = rest.split_whitespace().next().unwrap_or("");
            if typ == "redirect" {
                cur = Some((i, json::object::Object::new()));
            }
        } else if t.starts_with("option ") {
            if let Some((_, f)) = cur.as_mut() {
                let body = t["option ".len()..].trim();
                if let Some((k, v)) = body.split_once(char::is_whitespace) {
                    let v = v.trim().trim_matches('\'').trim_matches('"').to_string();
                    f.insert(k.trim(), v.into());
                }
            }
        }
    }
    if let Some((s, f)) = cur.take() {
        blocks.push(RedirectBlock { start: s, end: lines.len(), fields: f });
    }
    blocks
}

fn redirect_list_value(raw: &str) -> Vec<JsonValue> {
    scan_redirects(raw)
        .iter()
        .map(|b| {
            let mut m = json::object::Object::new();
            m.insert("name", b.fields.get("name").cloned().unwrap_or(JsonValue::Null));
            m.insert("proto", b.fields.get("proto").cloned().unwrap_or(JsonValue::Null));
            m.insert("src_dport", b.fields.get("src_dport").cloned().unwrap_or(JsonValue::Null));
            m.insert("dest_ip", b.fields.get("dest_ip").cloned().unwrap_or(JsonValue::Null));
            m.insert("dest_port", b.fields.get("dest_port").cloned().unwrap_or(JsonValue::Null));
            let en = b.fields.get("enabled").and_then(|v| v.as_str()).unwrap_or("1");
            m.insert("enabled", (if en == "0" { "false" } else { "true" }).into());
            m.insert("target", b.fields.get("target").cloned().unwrap_or(JsonValue::Null));
            JsonValue::Object(m)
        })
        .collect()
}

fn valid_rule_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn arg_port(v: &JsonValue) -> Option<u16> {
    let n = v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))?;
    if n < 1.0 || n > 65535.0 || n.fract() != 0.0 {
        return None;
    }
    Some(n as u16)
}

fn redirect_block_text(name: &str, proto: &str, sport: u16, ip: &str, dport: u16) -> String {
    format!(
        "config redirect\n      option target 'DNAT'\n      option name '{}'\n      option src 'wan'\n      option proto '{}'\n      option src_dport '{}'\n      option dest_ip '{}'\n      option dest_port '{}'\n      option family 'ipv4'\n",
        name, proto, sport, ip, dport
    )
}

fn write_firewall(content: &str) -> Result<(), String> {
    // Atomic replace: backup, write tmp on the same filesystem, rename.
    std::fs::copy(FW_PATH, FW_BACKUP).map_err(|e| format!("backup failed: {}", e))?;
    let tmp = "/etc/config/firewall.tmp";
    std::fs::write(tmp, content).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, FW_PATH).map_err(|e| e.to_string())
}

fn port_forwards(args: &JsonValue) -> JsonValue {
    let Some(action) = args["action"].as_str() else {
        return tool_error("action must be list, add or remove");
    };
    if !matches!(action, "list" | "add" | "remove") {
        return tool_error("action must be list, add or remove");
    }
    let raw = match std::fs::read_to_string(FW_PATH) {
        Ok(r) => r,
        Err(e) => return tool_error(format!("read firewall config failed: {}", e)),
    };

    if action == "list" {
        let mut res = json::object::Object::new();
        res.insert("redirects", JsonValue::Array(redirect_list_value(&raw)));
        return text_result(JsonValue::Object(res).to_string(), false);
    }

    let confirm = args["confirm"].as_bool().unwrap_or(false);
    if !confirm {
        return tool_error("refused: add/remove require confirm=true");
    }
    let Some(name) = args["name"].as_str() else {
        return tool_error("name is required");
    };
    if !valid_rule_name(name) {
        return tool_error("name must be 1-32 chars of [A-Za-z0-9_-]");
    }
    let blocks = scan_redirects(&raw);

    if action == "add" {
        if blocks.iter().any(|b| b.fields.get("name").and_then(|v| v.as_str()) == Some(name)) {
            return tool_error("a redirect with this name already exists");
        }
        let Some(proto) = args["proto"].as_str() else {
            return tool_error("proto must be tcp, udp or tcpudp");
        };
        if !matches!(proto, "tcp" | "udp" | "tcpudp") {
            return tool_error("proto must be tcp, udp or tcpudp");
        }
        let Some(sport) = arg_port(&args["src_dport"]) else {
            return tool_error("src_dport must be 1-65535");
        };
        let dport = if args["dest_port"].is_null() {
            sport
        } else {
            match arg_port(&args["dest_port"]) {
                Some(p) => p,
                None => return tool_error("dest_port must be 1-65535"),
            }
        };
        let Some(ip_str) = args["dest_ip"].as_str() else {
            return tool_error("dest_ip is required");
        };
        let Ok(ip) = ip_str.parse::<std::net::Ipv4Addr>() else {
            return tool_error("dest_ip must be an IPv4 address");
        };
        if !ip.is_private() {
            return tool_error("dest_ip must be a private (LAN) address");
        }
        let mut new_raw = raw.clone();
        if !new_raw.ends_with('\n') {
            new_raw.push('\n');
        }
        new_raw.push_str(&redirect_block_text(name, proto, sport, ip_str, dport));
        if let Err(e) = write_firewall(&new_raw) {
            return tool_error(format!("write firewall config failed: {}", e));
        }
    } else {
        let target = blocks.iter().find(|b| b.fields.get("name").and_then(|v| v.as_str()) == Some(name));
        let Some(b) = target else {
            return tool_error("no redirect with this name");
        };
        let lines: Vec<&str> = raw.lines().collect();
        let mut kept = String::new();
        for (i, l) in lines.iter().enumerate() {
            if i < b.start || i >= b.end {
                kept.push_str(l);
                kept.push('\n');
            }
        }
        if let Err(e) = write_firewall(&kept) {
            return tool_error(format!("write firewall config failed: {}", e));
        }
    }

    // The only exec in this tool: a literal reload.
    let (_, ok) = run_out(&mut Command::new("fw4").args(["reload"]));
    let mut res = json::object::Object::new();
    res.insert("applied", action.into());
    res.insert("name", name.into());
    res.insert("firewall", (if ok { "reloaded" } else { "reload failed; run fw4 reload manually" }).into());
    text_result(JsonValue::Object(res).to_string(), false)
}

fn network_traffic() -> JsonValue {
    fn read_net_dev() -> Vec<(String, u64, u64)> {
        let mut out: Vec<(String, u64, u64)> = Vec::new();
        if let Ok(s) = std::fs::read_to_string("/proc/net/dev") {
            for line in s.lines().skip(2) {
                if let Some((name, rest)) = line.split_once(':') {
                    let f: Vec<&str> = rest.split_whitespace().collect();
                    if f.len() >= 9 {
                        let rx = f[0].parse::<u64>().unwrap_or(0);
                        let tx = f[8].parse::<u64>().unwrap_or(0);
                        out.push((name.trim().to_string(), rx, tx));
                    }
                }
            }
        }
        out
    }
    let a = read_net_dev();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let b = read_net_dev();
    let mut ifaces: Vec<JsonValue> = Vec::new();
    for (name, brx, btx) in &b {
        let (arx, atx) = a
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, r, t)| (*r, *t))
            .unwrap_or((*brx, *btx));
        let mut m = json::object::Object::new();
        m.insert("name", name.clone().into());
        m.insert("rx_bytes", JsonValue::from(*brx as f64));
        m.insert("tx_bytes", JsonValue::from(*btx as f64));
        m.insert("rx_bytes_per_s", JsonValue::from(brx.saturating_sub(arx) as f64));
        m.insert("tx_bytes_per_s", JsonValue::from(btx.saturating_sub(atx) as f64));
        ifaces.push(JsonValue::Object(m));
    }
    let ct = std::fs::read_to_string("/proc/sys/net/netfilter/nf_conntrack_count")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(-1.0);
    let mut res = json::object::Object::new();
    res.insert("sample_seconds", JsonValue::from(1f64));
    res.insert("conntrack_count", JsonValue::from(ct));
    res.insert("interfaces", JsonValue::Array(ifaces));
    text_result(JsonValue::Object(res).to_string(), false)
}

fn package_list() -> JsonValue {
    let (raw, ok) = run_out(&mut Command::new("opkg").args(["list-installed"]));
    if !ok && raw.is_empty() {
        return tool_error("opkg list-installed failed");
    }
    let mut res = json::object::Object::new();
    res.insert("count", JsonValue::from(raw.lines().count() as f64));
    res.insert("packages", raw.into());
    text_result(JsonValue::Object(res).to_string(), false)
}

fn recent_logs(args: &JsonValue) -> JsonValue {
    let n = (args["lines"].as_f64().unwrap_or(50.0) as i64).clamp(1, 200) as usize;
    let (raw, ok) = run_out(&mut Command::new("logread"));
    if !ok && raw.is_empty() {
        return tool_error("logread failed");
    }
    let mut lines: Vec<&str> = raw.lines().collect();
    if lines.len() > n {
        lines = lines[lines.len() - n..].to_vec();
    }
    let mut res = json::object::Object::new();
    let arr: Vec<JsonValue> = lines.into_iter().map(JsonValue::from).collect();
    res.insert("lines", JsonValue::Array(arr));
    text_result(JsonValue::Object(res).to_string(), false)
}

fn reboot(args: &JsonValue) -> JsonValue {
    let confirm = args["confirm"].as_bool().unwrap_or(false);
    if !confirm {
        return tool_error("refused: reboot requires confirm=true");
    }
    let (_, ok) = run_out(&mut Command::new("reboot"));
    if !ok {
        return tool_error("reboot failed");
    }
    text_result(r#"{"status":"rebooting","note":"connection will drop"}"#.to_string(), false)
}

const SUPPORTED_PROTOS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

pub fn init_result(params: &JsonValue) -> JsonValue {
    let client = params["protocolVersion"].as_str().unwrap_or("");
    let proto = if SUPPORTED_PROTOS.contains(&client) {
        client
    } else {
        "2025-06-18"
    };

    let mut server_info = json::object::Object::new();
    server_info.insert("name", "router-mcp".into());
    server_info.insert("version", "0.4.0".into());

    let mut caps = json::object::Object::new();
    caps.insert("tools", JsonValue::Object(json::object::Object::new()));

    let mut res = json::object::Object::new();
    res.insert("protocolVersion", proto.into());
    res.insert("capabilities", JsonValue::Object(caps));
    res.insert("serverInfo", JsonValue::Object(server_info));
    res.insert(
        "instructions",
        "ImmortalWrt router MCP. Read: system_info, network_status, connected_devices, firewall_summary, recent_logs. Write: wifi_control (can disconnect clients), port_forwards (add/remove need confirm=true), reboot (needs confirm=true).".into(),
    );
    JsonValue::Object(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_name_accepts_only_safe_charset() {
        assert!(valid_rule_name("web-8080"));
        assert!(valid_rule_name("A_b-c1"));
        assert!(valid_rule_name(&"x".repeat(32)));
        assert!(!valid_rule_name(""));
        assert!(!valid_rule_name("has space"));
        // Newline in a name would break the UCI block structure — must be rejected.
        assert!(!valid_rule_name("bad\nname"));
        assert!(!valid_rule_name(&"x".repeat(33)));
    }

    #[test]
    fn port_arguments_accept_number_or_numeric_string() {
        assert_eq!(arg_port(&json::parse("8080").unwrap()), Some(8080));
        assert_eq!(arg_port(&json::parse("\"8080\"").unwrap()), Some(8080));
        assert_eq!(arg_port(&json::parse("1").unwrap()), Some(1));
        assert_eq!(arg_port(&json::parse("0").unwrap()), None);
        assert_eq!(arg_port(&json::parse("65536").unwrap()), None);
        assert_eq!(arg_port(&json::parse("1.5").unwrap()), None);
        assert_eq!(arg_port(&json::parse("\"abc\"").unwrap()), None);
    }

    #[test]
    fn redirect_block_is_uci_safe() {
        let t = redirect_block_text("web-8080", "tcp", 8080, "192.168.1.10", 80);
        assert!(t.starts_with("config redirect\n"));
        assert!(t.contains("option name 'web-8080'"));
        assert!(t.contains("option target 'DNAT'"));
        assert!(t.ends_with('\n'));
        assert!(!t.contains('\r'));
    }

    #[test]
    fn scan_redirects_finds_blocks_and_fields() {
        let raw = "config zone\n        option name 'wan'\n\nconfig redirect\n      option target 'DNAT'\n      option name 'test-rule'\n      option src 'wan'\n      option src_dport '8080'\n";
        let blocks = scan_redirects(raw);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].fields.get("name").and_then(|v| v.as_str()), Some("test-rule"));
        assert_eq!(blocks[0].fields.get("src_dport").and_then(|v| v.as_str()), Some("8080"));
        // The zone block before it must not be part of the redirect block.
        assert!(blocks[0].start > 0);
    }
}
