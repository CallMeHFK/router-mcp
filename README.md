# router-mcp

A minimal [Model Context Protocol](https://modelcontextprotocol.io) server that **runs on your OpenWrt/ImmortalWrt router**, so any MCP-capable agent (local or remote) can inspect and control it — with a security posture built for a device that already sits at the edge of your network.

- **Rust, ~550 KB static binary** (aarch64 musl, stripped, no runtime dependencies)
- **Two transports**: stdio (spawned over SSH — no new listening ports) and **Streamable HTTP** (for external agents)
- **Whitelisted tools only** — every `Command` argument is a compile-time literal; request data never reaches a shell
- **Fail-closed HTTP auth**, confirm gates on destructive actions, atomic config edits with automatic backup

## How it works

```
 ┌──────────────┐  SSH (stdio)    ┌─────────────────────────────┐
 │ local agent  ├────────────────►│ router: /usr/bin/router-mcp │
 └──────────────┘                 │  ├─ ubus / uci / iwinfo     │
 ┌──────────────┐  HTTP + token   │  ├─ /proc, /sys readers     │
 │ remote agent ├────────────────►│  └─ atomic firewall editor  │
 └──────────────┘                 └─────────────────────────────┘
```

The stdio mode is launched by the agent over SSH and opens **no listening sockets**. The HTTP mode listens on a configurable address (LAN-only by deployment convention) and requires a bearer token — it refuses to start without one on any non-loopback interface.

## Tools (10)

| Tool | Type | What it does |
|------|------|--------------|
| `system_info` | read | model, firmware, uptime, load, memory, overlay usage, SoC temperature |
| `network_status` | read | WAN/WAN6/LAN interfaces: up/down, addresses, gateway |
| `connected_devices` | read | DHCP leases, IPv6 neighbors, Wi-Fi associated stations |
| `network_traffic` | read | per-interface RX/TX counters + 1-second rate sample + conntrack count |
| `firewall_summary` | read | zones, policies, rule counts, port-forward details |
| `recent_logs` | read | last N lines of the system log (≤ 200) |
| `package_list` | read | all installed opkg packages |
| `port_forwards` | **write** | list/add/remove WAN port-forward (DNAT) rules — `add`/`remove` require `confirm: true` |
| `wifi_control` | **write** | enable/disable a Wi-Fi radio or read its state |
| `reboot` | **write** | reboot — requires `confirm: true` |

## Security model

1. **Literal-argument exec.** Every `std::process::Command` call site uses compile-time literal arguments. Request data is used exclusively as match keys for constant lookup — it is never interpolated into a command line, and no shell is ever spawned. The one exception that needs dynamic values (port-forward management) edits `/etc/config/firewall` **directly** with strict validation (name charset, proto enum, port range, private-IPv4-only destination), atomic tmp+rename writes, and an automatic pre-write backup; the only exec afterwards is a literal `fw4 reload`.
2. **Confirm gates.** Reboot, radio toggle, and port-forward add/remove require an explicit `confirm: true` from the calling agent.
3. **HTTP auth is fail-closed.** A non-loopback listener without a configured token refuses to start. Tokens come from an environment variable or a 0600 secret file — never from source, docs, or tests.
4. **No TLS by design.** Every Rust TLS stack carries build scripts (incompatible with Windows Smart App Control, see below). Encrypt the path with an SSH tunnel or a WireGuard-style overlay (e.g. Tailscale); plaintext + token on a trusted LAN is the documented trade-off.
5. **Dependency minimalism.** The only dependency is the [`json`](https://crates.io/crates/json) crate — no proc-macros, no build scripts, no `serde`.

> **Windows note (Smart App Control):** SAC blocks unsigned, locally generated build scripts, which rules out serde and most of the ecosystem. This project is deliberately structured so that the whole build runs only the signed Rust toolchain (rust-lld + self-contained musl), and the resulting ARM ELF never executes on the build host.

## Requirements

- OpenWrt 23.05+ / ImmortalWrt on `aarch64` (built and tested on ImmortalWrt 24.10, MediaTek MT7622)
- SSH key auth for the stdio transport (the HTTP transport replaces the key with its token)

## Quickstart

### 1. Get the binary

Build it (any host, no cross toolchain needed — the linker ships with Rust):

```bash
cargo build --release --target aarch64-unknown-linux-musl
# → target/aarch64-unknown-linux-musl/release/router-mcp (~550 KB)
```

…or grab a prebuilt `.ipk` from [Releases](../../releases) and skip to step 3.

### 2. Get it onto the router

```bash
scp -O target/aarch64-unknown-linux-musl/release/router-mcp root@192.168.1.1:/tmp/router-mcp
ssh root@192.168.1.1 'cp /tmp/router-mcp /usr/bin/router-mcp && chmod 755 /usr/bin/router-mcp'
# -O is required: dropbear has no sftp-server.
```

### 3a. stdio transport (local agent, e.g. Claude Code / ZCode)

```json
{
  "mcpServers": {
    "router": {
      "command": "ssh",
      "args": ["-o", "BatchMode=yes", "root@192.168.1.1", "/usr/bin/router-mcp"]
    }
  }
}
```

### 3b. Streamable HTTP transport (external agents)

On the router, install the `.ipk` (adds a procd service) or run it manually:

```bash
# generate a token once
tr -dc 'A-Za-z0-9' </dev/urandom | head -c 48 > /etc/router-mcp.secret && chmod 600 /etc/router-mcp.secret
# listen on the LAN address; refuses to start without a token
ROUTER_MCP_TOKEN_FILE=/etc/router-mcp.secret /usr/bin/router-mcp http 192.168.1.1:8420
```

Point the agent at `http://192.168.1.1:8420/mcp` with header `Authorization: Bearer <token>`. Single JSON responses are returned per POST; JSON-RPC batch arrays are accepted (≤ 16 per batch). `GET /mcp` returns 405 (no server-initiated SSE stream).

## Packaging as an .ipk (optional, recommended)

The OpenWrt-native way — `opkg install/remove`, visible in LuCI. **Build the package on the router itself**: a tarball made on Windows carries 644 modes and foreign UIDs, and MSYS `chmod`/`--mode` do not survive. The full procedure (staging tree, `control`/`conffiles`, legacy tar.gz ipk format) is a ~15-line script shown in [`AGENTS.md`](AGENTS.md).

## Interface naming

Wireless interface names vary by target (this project was built on a board where they are `wl0-ap0`/`wl1-ap0`, not `wlan0`/`wlan1`). Check yours with `iwinfo` and adjust the literals in `src/tools.rs` if needed — they are deliberately hardcoded per the security model.

## Development

```bash
cargo test --release    # unit tests (parsing, validation, UCI block scanning)
cargo build --release   # host binary
```

See [AGENTS.md](AGENTS.md) for the project overlay (exact commands, invariants, release-notes filter) and [CONTRIBUTING.md](CONTRIBUTING.md) for the PR flow. Security issues: [SECURITY.md](SECURITY.md) — please do not open public issues for vulnerabilities.

## Roadmap

- [ ] DHCP static lease management (same atomic-edit technique on `/etc/config/dhcp`)
- [ ] Kick a Wi-Fi client by MAC
- [ ] HTTP keep-alive and optional SSE stream
- [ ] Optional read-only MCP resources (config snapshots)

## License

[MIT](LICENSE) © CallMeHFK
