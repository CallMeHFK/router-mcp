# Changelog

All notable changes to this project are documented in this file. Format follows [Keep a Changelog](https://keepachangelog.com); versioning is [semantic](https://semver.org). Versions before 0.4.0 were developed privately and first published at 0.4.0.

## [0.4.0] - 2026-10-31

### Added

- `network_traffic` tool: per-interface RX/TX byte counters with a 1-second rate sample and conntrack entry count.
- `package_list` tool: all installed opkg packages with versions.
- `port_forwards` tool: list/add/remove WAN port-forward (DNAT) rules — validated values are written to `/etc/config/firewall` via atomic tmp+rename with an automatic pre-write backup, followed by a literal `fw4 reload`; `add`/`remove` require `confirm: true`, destinations are restricted to private IPv4.
- `firewall_summary` now includes full port-forward details (`redirect_details`).
- Streamable HTTP transport: JSON-RPC **batch array** support (≤ 16 per batch), complementing single-message POSTs.

### Changed

- Binary size 551 KB (from 517 KB) with the new tools; still a fully static musl ELF.

## [0.3.0] - 2026-10-31

### Added

- Streamable HTTP transport (`router-mcp http [<listen>]`): `POST /mcp` with bearer-token auth, fail-closed startup on non-loopback listeners without a token, procd service and `.ipk` packaging with conffile protection.
- 401/405/400/202 status semantics; connection-per-request, no keep-alive state.

## [0.2.0] - 2026-10-31

### Changed

- Replaced the Go prototype (2.8 MB) with a Rust implementation (447 KB): same 7-tool surface and security invariants (literal-argument exec, no shell), now on `rust-lld` + self-contained musl.

### Fixed

- UCI section-index parsing, radio interface naming (`wl0-ap0`/`wl1-ap0`), and Wi-Fi status semantics found during on-router validation.

## [0.1.0] - 2026-10-30

### Added

- Initial Go prototype: stdio MCP server with 7 whitelisted tools, deployed as a bare binary on ImmortalWrt 24.10 (Xiaomi AX6S / MT7622).
