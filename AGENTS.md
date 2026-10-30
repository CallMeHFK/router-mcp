# AGENTS.md — router-mcp project overlay

Project-specific overlay on [CallMeHFK/open-source-repo-rules](https://github.com/CallMeHFK/open-source-repo-rules). The shared rules define *what* must be true; this file says *how to verify it in this repository*. An overlay may tighten a shared rule but never weakens one.

## Stack

- Rust, edition 2021, stable toolchain.
- **Dependency policy (hard constraint):** `std` + the `json` crate only. Never add serde, proc-macros, or any dependency with a build script — Windows Smart App Control (SAC) blocks unsigned locally generated build scripts, so the build must run only the signed Rust toolchain.
- Target artifact: `aarch64-unknown-linux-musl` static ELF, soft size budget ≈ 1 MB.

## Exact commands

| Purpose | Command |
|---|---|
| Deploy artifact | `cargo build --release --target aarch64-unknown-linux-musl` |
| Host build | `cargo build --release` |
| Test | `cargo test --release` — **runs in CI; local execution of the test binary is blocked by SAC on Windows hosts** (verify locally with `cargo test --release --no-run`) |
| CI jobs (check-run names) | `build`, `test`, `cross-build` |

Cross-linking config lives in `.cargo/config.toml` (`linker = "rust-lld"`, `link-self-contained = yes`) — no zig, no musl-gcc, no VS Build Tools.

## Deployment (never part of CI)

- The `.ipk` **must be built on the router itself**: a tarball produced on Windows carries 644 modes and foreign UIDs, and MSYS `chmod` / `--mode` / `--owner` are unreliable. Procedure (staging tree, `control`, `conffiles`, legacy tar.gz ipk format) is documented in the README "Packaging" section.
- On-router paths: binary `/usr/bin/router-mcp`; service `/etc/init.d/router-mcp` (procd, `START=95`); config `/etc/config/router-mcp` (conffile — opkg preserves local edits and drops package copies as `*-opkg`); token `/etc/router-mcp.secret` (0600, auto-generated on first service start, never committed).
- File transfer to the router needs `scp -O` (dropbear ships no sftp-server).
- The uci config must use a **named** section (`config main 'main'`) — `config main` creates an anonymous section that `config_get x main y` cannot read.

## Code security invariants (review gate for every PR)

1. Every `std::process::Command` call site uses compile-time literal arguments only; request data reaches exec exclusively as match keys. No shell is ever spawned. File-editing tools (port-forward management) may embed strictly validated values into *file content* — never into argv.
2. No credentials, tokens, or secret material in source, tests, docs, or examples; runtime credentials come from environment variables or secret files only.
3. Destructive tools (`reboot`, radio `down`, port-forward `add`/`remove`) require an explicit `confirm: true` argument.
4. The HTTP listener is fail-closed: a non-loopback `listen` without a resolvable token aborts startup.
5. Wireless interface names (`wl0-ap0` / `wl1-ap0` on the reference board) are literals in `src/tools.rs` — changing hardware means changing those constants, deliberately.

## Branch / commit

- Default branch: `main`. Squash-merge only; **the PR title becomes the commit subject** and must satisfy Conventional Commits (`^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?!?: .+`).
- Solo maintainer, one forge account: branch protection runs `required_approving_review_count: 0`, `enforce_admins: true`; PRs are still mandatory.

## Release notes filter

- Internal-only (excluded from release bodies when non-breaking): `docs`, `style`, `refactor`, `ci`, `build`, `chore`, `test` — including scoped forms (`docs(x):`). Breaking forms (`type!:` / `type(scope)!:` / `BREAKING CHANGE:` footer) are always kept.
- `CHANGELOG.md` is the source of truth for release bodies; no standing `[Unreleased]` entry.
- Releases are cut from `main` with annotated tags `vX.Y.Z`; the release body is extracted from the matching `CHANGELOG.md` section.

## Multi-agent

- Single-agent repository (one maintainer, one forge account): ledger/lease/`INTEGRATION_REF` protocol from `multi-agent-rules.md` is not applicable. If a second agent is added, that rule fires and this section must be updated first.
- Test fixture surface: the unit tests in `src/tools.rs` (`cargo test --release`) cover rule-name validation, port parsing, UCI block scanning, and block-text generation.
