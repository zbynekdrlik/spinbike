# Prod & Dev Environments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the single-env SpinBike deployment into isolated prod (`spinbike.newlevel.media`) and dev (`spinbike-dev.newlevel.media`) environments on the same runner machine, with branch-gated CI deploys and pre-deploy DB backups for prod.

**Architecture:** Two systemd units (`spinbike.service`, `spinbike-dev.service`) on ports 8080/8081, separate binaries under `/opt/spinbike/{prod,dev}/`, separate DB files next to each binary, secrets moved from inline `Environment=` to `EnvironmentFile=/etc/default/spinbike-*`. CI's `deploy` job splits into `deploy-dev` (fires on push to `dev`) and `deploy-prod` (fires on push to `main`). Nightly systemd timer copies prod DB → dev DB. Cloudflare tunnel adds a second ingress rule.

**Tech Stack:** Bash, systemd, Cloudflare tunnel, GitHub Actions YAML. Zero Rust code changes. All version-controlled artifacts live under `deploy/` in the repo; actual secrets are generated on the runner and never enter git.

---

## Execution ordering constraint

The runner machine must have the new systemd units installed BEFORE any push to `dev` lands on CI — otherwise `deploy-dev` fails with "spinbike-dev.service: Unit not found". Sequence:

1. **Tasks 1–8** — repo file changes, committed to `dev` locally (not pushed yet).
2. **Task 9** — manual rollout on the runner (runs `scripts/setup-environments.sh`, updates Cloudflare). This moves prod from `/home/newlevel/devel/spinbike/target/release/` to `/opt/spinbike/prod/` and starts both services from the CURRENT prod binary.
3. **Task 10** — push to `dev`; `deploy-dev` runs against the now-ready dev env.
4. **Task 11** — PR `dev` → `main`, merge; `deploy-prod` runs and writes a fresh binary + DB backup to `/opt/spinbike/prod/`.
5. **Task 12** — post-rollout verification.

---

## File structure

**New files (committed to repo):**
- `deploy/systemd/spinbike.service` — replacement prod unit
- `deploy/systemd/spinbike-dev.service` — new dev unit
- `deploy/systemd/spinbike-sync-dev.service` — oneshot DB copy unit
- `deploy/systemd/spinbike-sync-dev.timer` — daily 03:00 trigger
- `deploy/systemd/spinbike-prod.env.example` — env template (no real secret)
- `deploy/systemd/spinbike-dev.env.example` — env template (no real secret)
- `deploy/cloudflared/config.yml.example` — tunnel ingress template
- `scripts/setup-environments.sh` — idempotent one-time rollout script
- `docs/operations/environments.md` — runbook

**Modified files:**
- `.github/workflows/ci.yml` — `deploy` job splits into `deploy-dev` + `deploy-prod`
- `VERSION` — 0.9.2 → 0.9.3

**Runner-machine artifacts (installed by Task 9, NEVER committed to git):**
- `/etc/systemd/system/spinbike.service` (overwrites existing)
- `/etc/systemd/system/spinbike-dev.service`
- `/etc/systemd/system/spinbike-sync-dev.service`
- `/etc/systemd/system/spinbike-sync-dev.timer`
- `/etc/default/spinbike-prod` (mode 0640, root:newlevel)
- `/etc/default/spinbike-dev` (mode 0640, root:newlevel)
- `/opt/spinbike/prod/` tree
- `/opt/spinbike/dev/` tree
- `~/.cloudflared/config.yml` (append ingress rule)

---

## Task 1: Bump VERSION

**Files:**
- Modify: `VERSION`

- [ ] **Step 1: Bump VERSION to 0.9.3**

```bash
echo 0.9.3 > VERSION
```

- [ ] **Step 2: Verify**

```bash
cat VERSION
```

Expected: `0.9.3`

- [ ] **Step 3: Sync into Cargo.toml files**

```bash
scripts/sync-version.sh
```

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml crates/*/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump VERSION to 0.9.3 for env-split work"
```

---

## Task 2: Create prod systemd unit template

**Files:**
- Create: `deploy/systemd/spinbike.service`

- [ ] **Step 1: Create the unit file**

Content of `deploy/systemd/spinbike.service`:

```ini
[Unit]
Description=SpinBike PWA Server (prod)
After=network.target

[Service]
Type=simple
User=newlevel
Environment=PORT=8080
Environment=DATABASE_PATH=/opt/spinbike/prod/spinbike.db
EnvironmentFile=/etc/default/spinbike-prod
ExecStart=/opt/spinbike/prod/spinbike-server
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

- [ ] **Step 2: Verify systemd-analyze accepts it**

```bash
systemd-analyze verify deploy/systemd/spinbike.service 2>&1 | grep -v "Cannot check" || true
```

Expected: no output (or only "Cannot check unit dependencies" warnings, which are harmless for offline validation).

- [ ] **Step 3: Commit**

```bash
git add deploy/systemd/spinbike.service
git commit -m "deploy: new prod systemd unit using /opt/spinbike/prod paths"
```

---

## Task 3: Create dev systemd unit

**Files:**
- Create: `deploy/systemd/spinbike-dev.service`

- [ ] **Step 1: Create the unit file**

Content of `deploy/systemd/spinbike-dev.service`:

```ini
[Unit]
Description=SpinBike PWA Server (dev)
After=network.target

[Service]
Type=simple
User=newlevel
Environment=PORT=8081
Environment=DATABASE_PATH=/opt/spinbike/dev/spinbike-dev.db
EnvironmentFile=/etc/default/spinbike-dev
ExecStart=/opt/spinbike/dev/spinbike-server
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

- [ ] **Step 2: Verify**

```bash
systemd-analyze verify deploy/systemd/spinbike-dev.service 2>&1 | grep -v "Cannot check" || true
```

Expected: no output / harmless warnings only.

- [ ] **Step 3: Commit**

```bash
git add deploy/systemd/spinbike-dev.service
git commit -m "deploy: dev systemd unit on port 8081 with separate DB path"
```

---

## Task 4: Create prod→dev sync unit + timer

**Files:**
- Create: `deploy/systemd/spinbike-sync-dev.service`
- Create: `deploy/systemd/spinbike-sync-dev.timer`

- [ ] **Step 1: Create the oneshot service**

Content of `deploy/systemd/spinbike-sync-dev.service`:

```ini
[Unit]
Description=Copy prod DB to dev and restart dev service
After=spinbike.service

[Service]
Type=oneshot
User=root
ExecStart=/usr/bin/cp /opt/spinbike/prod/spinbike.db /opt/spinbike/dev/spinbike-dev.db.tmp
ExecStart=/bin/chown newlevel:newlevel /opt/spinbike/dev/spinbike-dev.db.tmp
ExecStart=/usr/bin/mv -f /opt/spinbike/dev/spinbike-dev.db.tmp /opt/spinbike/dev/spinbike-dev.db
ExecStart=/usr/bin/systemctl restart spinbike-dev.service
```

- [ ] **Step 2: Create the timer**

Content of `deploy/systemd/spinbike-sync-dev.timer`:

```ini
[Unit]
Description=Nightly prod-to-dev DB sync at 03:00

[Timer]
OnCalendar=*-*-* 03:00:00
Persistent=true

[Install]
WantedBy=timers.target
```

- [ ] **Step 3: Verify both**

```bash
systemd-analyze verify deploy/systemd/spinbike-sync-dev.service 2>&1 | grep -v "Cannot check" || true
systemd-analyze verify deploy/systemd/spinbike-sync-dev.timer 2>&1 | grep -v "Cannot check" || true
```

Expected: no output / harmless warnings only.

- [ ] **Step 4: Commit**

```bash
git add deploy/systemd/spinbike-sync-dev.service deploy/systemd/spinbike-sync-dev.timer
git commit -m "deploy: nightly prod-to-dev DB sync via systemd timer"
```

---

## Task 5: Create env file templates

**Files:**
- Create: `deploy/systemd/spinbike-prod.env.example`
- Create: `deploy/systemd/spinbike-dev.env.example`

- [ ] **Step 1: Create prod env template**

Content of `deploy/systemd/spinbike-prod.env.example`:

```
# Copied to /etc/default/spinbike-prod by setup-environments.sh.
# The real JWT_SECRET is generated with `openssl rand -hex 32` at install
# time and written directly into /etc/default/spinbike-prod with mode 0640.
# This file is ONLY a template — NEVER put the real secret here.
JWT_SECRET=REPLACE_WITH_OPENSSL_RAND_HEX_32
CORS_ORIGIN=https://spinbike.newlevel.media
```

- [ ] **Step 2: Create dev env template**

Content of `deploy/systemd/spinbike-dev.env.example`:

```
# Copied to /etc/default/spinbike-dev by setup-environments.sh.
# The real JWT_SECRET is generated with `openssl rand -hex 32` at install
# time and written directly into /etc/default/spinbike-dev with mode 0640.
# This file is ONLY a template — NEVER put the real secret here.
JWT_SECRET=REPLACE_WITH_OPENSSL_RAND_HEX_32
CORS_ORIGIN=https://spinbike-dev.newlevel.media
```

- [ ] **Step 3: Commit**

```bash
git add deploy/systemd/spinbike-prod.env.example deploy/systemd/spinbike-dev.env.example
git commit -m "deploy: env file templates for prod and dev (real secrets never in git)"
```

---

## Task 6: Create Cloudflare tunnel config template

**Files:**
- Create: `deploy/cloudflared/config.yml.example`

- [ ] **Step 1: Create the template**

Content of `deploy/cloudflared/config.yml.example`:

```yaml
# Template for ~/.cloudflared/config.yml. The real file lives on the runner
# alongside the tunnel credentials JSON. Ingress rules match top-down;
# http_status:404 is always the catch-all fallback for unknown hostnames.
tunnel: 4093c494-b31d-4eb7-8fcb-6c5948f5d4b2
credentials-file: /home/newlevel/.cloudflared/4093c494-b31d-4eb7-8fcb-6c5948f5d4b2.json

ingress:
  - hostname: spinbike.newlevel.media
    service: http://localhost:8080
  - hostname: spinbike-dev.newlevel.media
    service: http://localhost:8081
  - service: http_status:404
```

- [ ] **Step 2: Commit**

```bash
git add deploy/cloudflared/config.yml.example
git commit -m "deploy: Cloudflare tunnel config template with both env ingress rules"
```

---

## Task 7: Create setup-environments.sh

**Files:**
- Create: `scripts/setup-environments.sh`

- [ ] **Step 1: Write the script**

Content of `scripts/setup-environments.sh`:

```bash
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

echo "==> Creating /opt/spinbike directory tree"
sudo install -d -o newlevel -g newlevel "$PROD_DIR" "$PROD_DIR/backups" "$DEV_DIR"

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

echo "==> Done. Next steps (manual, require Cloudflare auth):"
echo "  1. Edit ~/.cloudflared/config.yml per deploy/cloudflared/config.yml.example"
echo "  2. cloudflared tunnel route dns spinbike spinbike-dev.newlevel.media"
echo "  3. sudo systemctl restart spinbike-tunnel.service"
```

- [ ] **Step 2: Make executable**

```bash
chmod +x scripts/setup-environments.sh
```

- [ ] **Step 3: Syntax-check the shell script**

```bash
bash -n scripts/setup-environments.sh
```

Expected: no output (exit 0).

- [ ] **Step 4: Shellcheck (if available)**

```bash
command -v shellcheck >/dev/null && shellcheck scripts/setup-environments.sh || echo "shellcheck not installed, skip"
```

Expected: no warnings, or "shellcheck not installed, skip".

- [ ] **Step 5: Commit**

```bash
git add scripts/setup-environments.sh
git commit -m "deploy: idempotent one-time env setup script"
```

---

## Task 8: Split CI deploy job

**Files:**
- Modify: `.github/workflows/ci.yml:222-273`

- [ ] **Step 1: Replace the single `deploy` job with `deploy-dev` and `deploy-prod`**

Delete lines 222–273 (the entire existing `deploy:` job) and replace with:

```yaml
  deploy-dev:
    name: Deploy (dev)
    runs-on: [self-hosted, spinbike-deploy]
    needs: [test, build-wasm, e2e]
    timeout-minutes: 30
    if: github.event_name == 'push' && github.ref == 'refs/heads/dev'
    steps:
      - uses: actions/checkout@v4

      - name: Build WASM frontend (Trunk)
        run: |
          cd spinbike-ui
          trunk build --release

      - name: Force rust-embed to re-bake dist
        run: touch crates/spinbike-server/src/routes/static_files.rs

      - name: Build release server
        run: cargo build --release --bin spinbike-server

      - name: Install dev binary and restart dev service
        run: |
          install -Dm 755 target/release/spinbike-server \
            /opt/spinbike/dev/spinbike-server
          sudo -n systemctl restart spinbike-dev.service

      - name: Wait for dev site health
        run: |
          for i in $(seq 1 30); do
            if curl -sfI https://spinbike-dev.newlevel.media > /dev/null; then
              echo "Dev site responding (attempt $i)"
              exit 0
            fi
            sleep 2
          done
          echo "ERROR: dev site did not respond within 60s"
          sudo -n systemctl status spinbike-dev.service --no-pager || true
          exit 1

      - name: Post-deploy smoke tests (Playwright against dev)
        run: |
          cd e2e
          npm ci
          npx playwright install --with-deps chromium
          SMOKE_BASE_URL=https://spinbike-dev.newlevel.media \
            npx playwright test -g '@smoke'

  deploy-prod:
    name: Deploy (prod)
    runs-on: [self-hosted, spinbike-deploy]
    needs: [test, build-wasm, e2e]
    timeout-minutes: 30
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4

      - name: Backup prod DB (timestamped, keep last 10)
        run: |
          ts=$(date +%Y%m%d-%H%M%S)
          cp /opt/spinbike/prod/spinbike.db \
             /opt/spinbike/prod/backups/spinbike-${ts}.db
          # Keep only the 10 most recent; delete older ones.
          ls -t /opt/spinbike/prod/backups/spinbike-*.db 2>/dev/null \
            | tail -n +11 \
            | xargs -r rm -f
          echo "Backup written: spinbike-${ts}.db"
          ls -lh /opt/spinbike/prod/backups/

      - name: Build WASM frontend (Trunk)
        run: |
          cd spinbike-ui
          trunk build --release

      - name: Force rust-embed to re-bake dist
        run: touch crates/spinbike-server/src/routes/static_files.rs

      - name: Build release server
        run: cargo build --release --bin spinbike-server

      - name: Install prod binary and restart prod service
        run: |
          install -Dm 755 target/release/spinbike-server \
            /opt/spinbike/prod/spinbike-server
          sudo -n systemctl restart spinbike.service

      - name: Wait for prod site health
        run: |
          for i in $(seq 1 30); do
            if curl -sfI https://spinbike.newlevel.media > /dev/null; then
              echo "Prod site responding (attempt $i)"
              exit 0
            fi
            sleep 2
          done
          echo "ERROR: prod site did not respond within 60s"
          sudo -n systemctl status spinbike.service --no-pager || true
          exit 1

      - name: Post-deploy smoke tests (Playwright against prod)
        run: |
          cd e2e
          npm ci
          npx playwright install --with-deps chromium
          SMOKE_BASE_URL=https://spinbike.newlevel.media \
            npx playwright test -g '@smoke'
```

- [ ] **Step 2: Verify YAML is syntactically valid**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo OK
```

Expected: `OK`

- [ ] **Step 3: Verify the branch guards exist**

```bash
grep -E "refs/heads/(main|dev)" .github/workflows/ci.yml
```

Expected: two matches — one for `refs/heads/dev` (deploy-dev) and one for `refs/heads/main` (deploy-prod).

- [ ] **Step 4: Verify the prod backup step exists**

```bash
grep -c "Backup prod DB" .github/workflows/ci.yml
```

Expected: `1`

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: split deploy job into dev (branch dev) and prod (branch main)

Adds pre-deploy DB backup to prod with 10-item rolling retention.
Dev deploys target port 8081 / spinbike-dev.newlevel.media; prod stays
on port 8080 / spinbike.newlevel.media. Both read binary paths from
/opt/spinbike/{dev,prod}/ so the two envs never overwrite each other."
```

---

## Task 9: Write operations runbook

**Files:**
- Create: `docs/operations/environments.md`

- [ ] **Step 1: Write the runbook**

Content of `docs/operations/environments.md`:

````markdown
# SpinBike Environments Runbook

Two environments run on the same machine:

| Env  | URL                                 | Port | Service                | DB                                         | Branch |
|------|-------------------------------------|------|------------------------|--------------------------------------------|--------|
| prod | https://spinbike.newlevel.media     | 8080 | spinbike.service       | /opt/spinbike/prod/spinbike.db             | main   |
| dev  | https://spinbike-dev.newlevel.media | 8081 | spinbike-dev.service   | /opt/spinbike/dev/spinbike-dev.db          | dev    |

## One-time rollout

See `scripts/setup-environments.sh`. Run once as the `newlevel` user on the
runner machine:

```bash
./scripts/setup-environments.sh
```

Then do the Cloudflare steps manually (need Cloudflare auth):

```bash
# Edit ~/.cloudflared/config.yml per deploy/cloudflared/config.yml.example
cloudflared tunnel route dns spinbike spinbike-dev.newlevel.media
sudo systemctl restart spinbike-tunnel.service
```

## Daily operations

- Push to `dev` → CI auto-deploys to dev env. Safe to push broken code here.
- Merge `dev` → `main` → CI auto-deploys to prod env after running DB backup.
- Nightly at 03:00, `spinbike-sync-dev.timer` copies the prod DB over the dev
  DB so dev tests against realistic data.

## Inspecting service state

```bash
systemctl status spinbike.service spinbike-dev.service
journalctl -u spinbike.service -n 100
journalctl -u spinbike-dev.service -n 100
```

## Backups

Pre-deploy snapshots live in `/opt/spinbike/prod/backups/` as
`spinbike-YYYYMMDD-HHMMSS.db`. CI keeps the last 10.

### Restore from backup

```bash
sudo systemctl stop spinbike.service
sudo cp /opt/spinbike/prod/backups/spinbike-<ts>.db /opt/spinbike/prod/spinbike.db
sudo chown newlevel:newlevel /opt/spinbike/prod/spinbike.db
sudo systemctl start spinbike.service
```

## Secret rotation

```bash
# Generate new secret
NEW=$(openssl rand -hex 32)
sudo sed -i "s|^JWT_SECRET=.*|JWT_SECRET=$NEW|" /etc/default/spinbike-prod
sudo systemctl restart spinbike.service
```

All existing user sessions invalidate on the next request — expected.

## Rollback (forward-fix)

There is no automated binary rollback. To revert a bad prod deploy:

1. Push a revert commit to `dev` (`git revert <sha>`).
2. Merge `dev` → `main`.
3. CI redeploys the reverted code to prod.

If prod DB was corrupted by the bad deploy, restore from backup (see above)
BEFORE redeploying, so migrations run against the clean snapshot.
````

- [ ] **Step 2: Commit**

```bash
git add docs/operations/environments.md
git commit -m "docs: environments runbook"
```

---

## Task 10: Execute one-time rollout on the runner (MANUAL)

**This task runs on the runner machine, not via CI. Do NOT push before this task completes.**

- [ ] **Step 1: Ensure a fresh release binary exists**

```bash
cd /home/newlevel/devel/spinbike
cd spinbike-ui && trunk build --release && cd ..
touch crates/spinbike-server/src/routes/static_files.rs
cargo build --release --bin spinbike-server
```

Expected: `/home/newlevel/devel/spinbike/target/release/spinbike-server` exists and is newer than `spinbike.db`.

- [ ] **Step 2: Run setup script**

```bash
./scripts/setup-environments.sh
```

Expected final line: `Done. Next steps (manual, require Cloudflare auth):`

- [ ] **Step 3: Verify both services are running**

```bash
systemctl is-active spinbike.service spinbike-dev.service
curl -sf http://localhost:8080 | head -1
curl -sf http://localhost:8081 | head -1
```

Expected: both services `active`, both curl commands return an HTML line (likely starting with `<!doctype html>`).

- [ ] **Step 4: Verify timer is armed**

```bash
systemctl is-active spinbike-sync-dev.timer
systemctl list-timers spinbike-sync-dev.timer --no-pager
```

Expected: `active`, and the list shows the next fire time at 03:00.

- [ ] **Step 5: Update Cloudflare tunnel**

```bash
# Backup existing config
cp ~/.cloudflared/config.yml ~/.cloudflared/config.yml.bak
```

Edit `~/.cloudflared/config.yml` to match `deploy/cloudflared/config.yml.example` (add the `spinbike-dev.newlevel.media` ingress rule before the `http_status:404` catch-all).

```bash
cloudflared tunnel route dns spinbike spinbike-dev.newlevel.media
sudo systemctl restart spinbike-tunnel.service
```

- [ ] **Step 6: Verify both hostnames resolve and respond**

```bash
curl -sI https://spinbike.newlevel.media | head -1
curl -sI https://spinbike-dev.newlevel.media | head -1
```

Expected: both return `HTTP/2 200` (may need up to 30 s for DNS propagation on the new hostname).

- [ ] **Step 7: Verify with Playwright against both URLs**

```bash
cd e2e
SMOKE_BASE_URL=https://spinbike.newlevel.media npx playwright test -g '@smoke'
SMOKE_BASE_URL=https://spinbike-dev.newlevel.media npx playwright test -g '@smoke'
```

Expected: both smoke suites pass.

---

## Task 11: Push to dev and verify CI deploy-dev

- [ ] **Step 1: Push the accumulated commits**

```bash
git push origin dev
```

- [ ] **Step 2: Watch the CI run**

```bash
sleep 60 && gh run list --branch dev --limit 1 --json databaseId,status,conclusion,displayTitle
```

Find the run ID, then:

```bash
gh run view <run-id> --json status,conclusion,jobs \
  --jq '{status, conclusion, jobs: [.jobs[] | {name, status, conclusion}]}'
```

Poll with `sleep 300 && gh run view <run-id>` (background) until terminal.

- [ ] **Step 3: Verify `deploy-dev` succeeded and `deploy-prod` was skipped**

```bash
gh run view <run-id> --json jobs --jq '.jobs[] | {name, conclusion}'
```

Expected: `deploy-dev` → `success`. `deploy-prod` → `skipped` (wrong branch). All other jobs green.

- [ ] **Step 4: Verify dev site now serves the fresh binary**

```bash
curl -s https://spinbike-dev.newlevel.media/api/version 2>/dev/null || \
  curl -sI https://spinbike-dev.newlevel.media | head -1
```

Expected: HTTP/2 200. Binary at `/opt/spinbike/dev/spinbike-server` is now the CI-built version (mtime within the last few minutes).

```bash
ssh-free check on runner:  ls -l /opt/spinbike/dev/spinbike-server
```

Expected: mtime within the last ~10 minutes.

---

## Task 12: PR dev → main and verify deploy-prod

- [ ] **Step 1: Create PR**

```bash
gh pr create --base main --head dev \
  --title "Split prod & dev environments" \
  --body "$(cat <<'EOF'
## Summary
- Two systemd units on separate ports (prod 8080, dev 8081), separate DBs
- CI `deploy` job splits into `deploy-dev` (push to dev) and `deploy-prod` (push to main)
- Pre-deploy DB backup for prod with 10-snapshot rolling retention
- Nightly prod→dev DB sync via systemd timer
- Secrets moved from inline `Environment=` to `/etc/default/spinbike-*`

## Test plan
- [x] `setup-environments.sh` ran successfully on runner (Task 10)
- [x] Both URLs serve the app (spinbike.newlevel.media, spinbike-dev.newlevel.media)
- [x] `deploy-dev` succeeded on dev-branch push, `deploy-prod` skipped (Task 11)
- [ ] After merge: `deploy-prod` succeeds, timestamped backup lands in /opt/spinbike/prod/backups/, prod URL serves fresh binary

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Wait for PR CI to turn green**

```bash
gh pr checks <pr-number>
```

All required checks must be `success`.

- [ ] **Step 3: Verify PR is mergeable**

```bash
gh api repos/zbynekdrlik/spinbike/pulls/<pr-number> --jq '{mergeable, mergeable_state}'
```

Expected: `{"mergeable": true, "mergeable_state": "clean"}`.

- [ ] **Step 4: STOP — wait for user to say "merge it"**

Do NOT merge. Send the PR URL. Per airuleset `pr-merge-policy`, only an explicit user instruction can trigger the merge.

- [ ] **Step 5: After user approval, merge**

```bash
gh pr merge <pr-number> --merge
```

- [ ] **Step 6: Monitor main-branch CI**

```bash
sleep 60 && gh run list --branch main --limit 1 --json databaseId,status,conclusion
```

Then `gh run view <run-id> --json jobs` and poll until terminal.

- [ ] **Step 7: Verify `deploy-prod` succeeded and produced a backup**

```bash
gh run view <run-id> --log --job $(gh run view <run-id> --json jobs --jq '.jobs[] | select(.name=="Deploy (prod)").databaseId')
```

Look for the `Backup written: spinbike-...db` line in the log. Then on the runner:

```bash
ls -lh /opt/spinbike/prod/backups/ | tail -3
```

Expected: a file `spinbike-YYYYMMDD-HHMMSS.db` with a mtime from the deploy.

- [ ] **Step 8: Verify prod site serves the freshly built binary**

```bash
ls -l /opt/spinbike/prod/spinbike-server
curl -sI https://spinbike.newlevel.media | head -1
```

Expected: binary mtime within last ~10 minutes; HTTP/2 200.

- [ ] **Step 9: Playwright smoke against live prod**

```bash
cd e2e && SMOKE_BASE_URL=https://spinbike.newlevel.media npx playwright test -g '@smoke'
```

Expected: PASS.

- [ ] **Step 10: Final sanity — functional check of a real flow**

Open https://spinbike.newlevel.media in Playwright, log in with a known account, search for a known card, verify the data matches what was in the DB before the migration. (Autonomous-verification rule: functional test, not just liveness.)

---

## Self-review checklist

**Spec coverage:**

| Spec section                | Task(s)     |
|-----------------------------|-------------|
| File & process layout       | 2, 3, 7, 10 |
| Reverse proxy (Cloudflare)  | 6, 10       |
| Systemd units               | 2, 3, 4, 7  |
| Nightly prod→dev sync       | 4, 7, 10    |
| CI deploy split             | 8, 11, 12   |
| Existing data migration     | 7, 10       |
| Secrets via /etc/default    | 5, 7, 9     |
| Error handling (backups, journal)| 8, 9   |
| Rollout risk mitigation     | 10, 12      |

No gaps.

**Placeholder scan:** no TBDs. No "similar to Task N" — each task repeats its full code. No "handle edge cases" hand-waves. All commands are concrete.

**Type consistency:**
- `/opt/spinbike/prod/` and `/opt/spinbike/dev/` paths are consistent across tasks 2, 3, 4, 7, 8, 9.
- Service names `spinbike.service`, `spinbike-dev.service`, `spinbike-sync-dev.{service,timer}` consistent.
- Env file paths `/etc/default/spinbike-{prod,dev}` consistent.
- Port numbers 8080 (prod) / 8081 (dev) consistent.
- `SMOKE_BASE_URL` env var matches existing CI convention (spec section 3 + Task 8 + Task 10).
