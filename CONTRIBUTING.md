# Contributing

Thanks for considering a contribution. This repo follows the [open-source-repo-rules](https://github.com/CallMeHFK/open-source-repo-rules); the short version below, the project overlay in [AGENTS.md](AGENTS.md) for the exact commands and invariants.

## Dev setup

- Rust stable (edition 2021). No system dependencies — the cross build uses the bundled `rust-lld` and self-contained musl (`rustup target add aarch64-unknown-linux-musl` is all you need).
- Windows + Smart App Control: this repo is deliberately free of proc-macros and build scripts so the build runs entirely on the signed toolchain. **Please keep it that way** — PRs introducing serde or any build-script dependency cannot be built in that environment.
- Reference target: aarch64 OpenWrt/ImmortalWrt. Wireless interface names differ per board; they are hardcoded literals by design (see `src/tools.rs`).

## Build & test

```bash
cargo build --release --target aarch64-unknown-linux-musl   # the deploy artifact
cargo test --release                                        # unit tests (run in CI)
```

Windows + Smart App Control hosts cannot execute the locally built test binary (unsigned) — verify with `cargo test --release --no-run` and let CI run the tests.

## Pull requests

- **Title = Conventional Commit** (`feat: …`, `fix(router): …`) — under squash-merge the PR title becomes the commit subject on `main`.
- `main` is protected: every change enters via PR. Force pushes and direct pushes are disabled for everyone, including the maintainer.
- Keep PRs focused; the reviewer is also the maintainer, so small diffs get merged faster.
- CI runs `build`, `test`, and `cross-build` on every PR — keep them green.

## Contributor origin terms

None — no DCO sign-off and no CLA. By opening a PR you assert the change is yours to contribute under the [MIT license](LICENSE).

## Security

Do **not** open public issues for vulnerabilities — see [SECURITY.md](SECURITY.md) for the private reporting channel.
