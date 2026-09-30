#!/usr/bin/env bash
# Cross-compiles the homeguard-agent binary for every supported poste platform
# and stages the results in app/src-tauri/resources/agents/, where Tauri picks
# them up as bundle resources (see app/src-tauri/tauri.conf.json "resources").
#
# Requirements (macOS host):
#   brew install zig mingw-w64          # native Apple Silicon prefix: /opt/homebrew
#   cargo install cargo-zigbuild
#   rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-pc-windows-gnu
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/app/src-tauri/resources/agents"
mkdir -p "$OUT_DIR"

cd "$ROOT_DIR"

echo "==> Linux x86_64 (musl)"
cargo zigbuild --release --target x86_64-unknown-linux-musl -p homeguard-agent
cp "target/x86_64-unknown-linux-musl/release/homeguard-agent" "$OUT_DIR/homeguard-agent-linux-x86_64"
chmod +x "$OUT_DIR/homeguard-agent-linux-x86_64"

echo "==> Linux aarch64 (musl)"
cargo zigbuild --release --target aarch64-unknown-linux-musl -p homeguard-agent
cp "target/aarch64-unknown-linux-musl/release/homeguard-agent" "$OUT_DIR/homeguard-agent-linux-aarch64"
chmod +x "$OUT_DIR/homeguard-agent-linux-aarch64"

echo "==> Windows x86_64 (gnu)"
cargo build --release --target x86_64-pc-windows-gnu -p homeguard-agent
cp "target/x86_64-pc-windows-gnu/release/homeguard-agent.exe" "$OUT_DIR/homeguard-agent-windows-x86_64.exe"

# All three targets are built from the same homeguard-agent package, so
# one version covers all of them. Read it straight from the workspace
# manifest rather than asking cargo (avoids depending on jq/python being
# installed just to parse `cargo metadata`'s JSON) — `[workspace.package]`
# is the only top-level `version = "..."` line in this file.
AGENT_VERSION=$(grep -m1 '^version = ' "$ROOT_DIR/Cargo.toml" | sed -E 's/^version = "(.*)"/\1/')
echo "$AGENT_VERSION" > "$OUT_DIR/version.txt"

echo "==> Done, staged in $OUT_DIR (version $AGENT_VERSION)"
ls -la "$OUT_DIR"
