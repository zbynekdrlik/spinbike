#!/usr/bin/env bash
# Idempotent one-time rollout of the two-env layout on the runner machine.
# Safe to re-run; each step checks current state and skips work already done.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
PROD_DIR=/opt/spinbike/prod
DEV_DIR=/opt/spinbike/dev
OLD_DB=/home/newlevel/devel/spinbike/spinbike.db
OLD_BIN=/home/newlevel/devel/spinbike/target/release/spinbike-server

require_command() {
    command -v "$1" >/dev/null 2>&1 || { echo "ERROR: $1 not found"; exit 1; }
}

require_command systemctl
require_command openssl
require_command install
require_command sqlite3

echo "==> Creating /opt/spinbike directory tree"
sudo install -d -o newlevel -g newlevel "$PROD_DIR" "$DEV_DIR"
# Backups hold customer data — tighten to owner-only access.
sudo install -d -o newlevel -g newlevel -m 0700 "$PROD_DIR/backups"

if [ ! -f "$PROD_DIR/spinbike.db" ]; then
    echo "==> Copying existing prod DB to $PROD_DIR"
    [ -f "$OLD_DB" ] || { echo "ERROR: $OLD_DB missing"; exit 1; }
    sudo systemctl stop spinbike.service || true
    sudo cp "$OLD_DB" "$PROD_DIR/spinbike.db"
    sudo chown newlevel:newlevel "$PROD_DIR/spinbike.db"
else
    echo "==> Prod DB already at $PROD_DIR/spinbike.db (skip)"
fi

if [ ! -f "$DEV_DIR/spinbike-dev.db" ]; then
    echo "==> Seeding dev DB from prod"
    sudo cp "$PROD_DIR/spinbike.db" "$DEV_DIR/spinbike-dev.db"
    sudo chown newlevel:newlevel "$DEV_DIR/spinbike-dev.db"
else
    echo "==> Dev DB already at $DEV_DIR/spinbike-dev.db (skip)"
fi

echo "==> Installing bootstrap binary into both env dirs"
[ -f "$OLD_BIN" ] || { echo "ERROR: $OLD_BIN missing — build prod binary first"; exit 1; }
sudo install -Dm755 "$OLD_BIN" "$PROD_DIR/spinbike-server"
sudo install -Dm755 "$OLD_BIN" "$DEV_DIR/spinbike-server"

ensure_env_file() {
    local dest="$1"
    local cors_origin="$2"
    if [ ! -f "$dest" ]; then
        echo "==> Generating $dest with fresh JWT_SECRET"
        local secret
        secret="$(openssl rand -hex 32)"
        sudo tee "$dest" >/dev/null <<EOF
JWT_SECRET=$secret
CORS_ORIGIN=$cors_origin
EOF
        sudo chown root:newlevel "$dest"
        sudo chmod 0640 "$dest"
    else
        echo "==> $dest already exists (skip)"
    fi
}

ensure_env_file /etc/default/spinbike-prod https://spinbike.newlevel.media
ensure_env_file /etc/default/spinbike-dev https://spinbike-dev.newlevel.media

echo "==> Installing systemd unit files"
sudo install -Dm644 "$REPO/deploy/systemd/spinbike.service"          /etc/systemd/system/spinbike.service
sudo install -Dm644 "$REPO/deploy/systemd/spinbike-dev.service"      /etc/systemd/system/spinbike-dev.service
sudo install -Dm644 "$REPO/deploy/systemd/spinbike-sync-dev.service" /etc/systemd/system/spinbike-sync-dev.service
sudo install -Dm644 "$REPO/deploy/systemd/spinbike-sync-dev.timer"   /etc/systemd/system/spinbike-sync-dev.timer

echo "==> Reloading systemd and starting services"
sudo systemctl daemon-reload
sudo systemctl enable --now spinbike.service spinbike-dev.service spinbike-sync-dev.timer

echo "==> Waiting for services to come up"
for i in $(seq 1 15); do
    if curl -sf http://localhost:8080 >/dev/null && curl -sf http://localhost:8081 >/dev/null; then
        echo "Both services responding locally."
        break
    fi
    [ "$i" -eq 15 ] && { echo "ERROR: services failed to respond on 8080/8081"; exit 1; }
    sleep 2
done

echo "==> Checking for orphan cloudflared processes"
# A non-systemd `cloudflared tunnel run spinbike` racing the managed one will
# silently serve stale ingress config and break new hostnames with 404s.
systemd_pid=$(systemctl show spinbike-tunnel.service -p MainPID --value || echo 0)
extras=$(pgrep -af 'cloudflared tunnel run spinbike' | awk -v pid="$systemd_pid" '$1 != pid' | wc -l)
if [ "$extras" -gt 0 ]; then
    echo "WARN: $extras non-systemd cloudflared process(es) detected:"
    pgrep -af 'cloudflared tunnel run spinbike' | awk -v pid="$systemd_pid" '$1 != pid'
    echo "     Kill them to avoid ingress drift: kill <pid>"
fi

echo "==> Done. Next steps (manual, require Cloudflare auth):"
echo "  1. Edit ~/.cloudflared/config.yml per deploy/cloudflared/config.yml.example"
echo "  2. cloudflared tunnel route dns spinbike spinbike-dev.newlevel.media"
echo "  3. sudo systemctl restart spinbike-tunnel.service"
