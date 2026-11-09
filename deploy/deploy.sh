#!/usr/bin/env bash
# Builds the client and the server locally and deploys both to the host.
# Usage: deploy/deploy.sh [user@host]
set -euo pipefail

HOST="${1:-azl@90.156.203.211}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=x86_64-unknown-linux-musl

cd "$ROOT/client"
trunk build --release

cd "$ROOT"
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  cargo build --release -p chroma-server --target "$TARGET"

rsync -az --delete "$ROOT/client/dist/" "$HOST:/var/www/chroma-siege/"
rsync -az "$ROOT/target/$TARGET/release/chroma-server" "$ROOT/deploy/chroma-server.service" "$ROOT/deploy/Caddyfile" "$HOST:/tmp/chroma-deploy/"

ssh "$HOST" 'set -euo pipefail
sudo install -d -m 755 /opt/chroma-siege
sudo install -m 755 /tmp/chroma-deploy/chroma-server /opt/chroma-siege/chroma-server
sudo install -m 644 /tmp/chroma-deploy/chroma-server.service /etc/systemd/system/chroma-server.service
sudo install -m 644 /tmp/chroma-deploy/Caddyfile /etc/caddy/Caddyfile
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl daemon-reload
sudo systemctl enable --now chroma-server
sudo systemctl restart chroma-server
sudo systemctl reload caddy
rm -rf /tmp/chroma-deploy
systemctl is-active chroma-server caddy'
