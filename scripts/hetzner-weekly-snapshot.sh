#!/usr/bin/env bash
# Weekly Hetzner snapshot of the SpinBike VPS, 4-snapshot retention (#369).
# Runs as root from spinbike-weekly-snapshot.timer; token is read from a
# mode-600 file and never logged.
set -euo pipefail

TOKEN_FILE=/home/newlevel/.secrets/hetzner-spinbike
SERVER_ID=161810377
PREFIX=spinbike-weekly-
KEEP=4
API=https://api.hetzner.cloud/v1

token=$(tr -d '[:space:]' < "$TOKEN_FILE")
auth=(-H "Authorization: Bearer $token")

desc="${PREFIX}$(date +%Y-%m-%d)"
create=$(curl -fsS -X POST "$API/servers/$SERVER_ID/actions/create_image" \
  "${auth[@]}" -H "Content-Type: application/json" \
  -d "{\"type\":\"snapshot\",\"description\":\"$desc\"}")
image_id=$(jq -r '.image.id' <<<"$create")
action_id=$(jq -r '.action.id' <<<"$create")
echo "created snapshot $desc (image $image_id, action $action_id)"

# A snapshot can take minutes; wait up to 30 min for the action to finish.
status=running
for _ in $(seq 1 180); do
  status=$(curl -fsS "$API/actions/$action_id" "${auth[@]}" | jq -r '.action.status')
  [ "$status" = success ] && break
  if [ "$status" = error ]; then
    echo "snapshot action $action_id failed" >&2
    exit 1
  fi
  sleep 10
done
if [ "$status" != success ]; then
  echo "snapshot action $action_id not finished after 30 min (status: $status)" >&2
  exit 1
fi
echo "snapshot $image_id ready"

# Prune: keep the newest $KEEP snapshots carrying our prefix; never touch others.
curl -fsS "$API/images?type=snapshot&per_page=50" "${auth[@]}" \
  | jq -r --arg p "$PREFIX" --argjson keep "$KEEP" \
      '.images | map(select(.description | startswith($p))) | sort_by(.created) | reverse | .[$keep:] | .[].id' \
  | while read -r old; do
      [ -n "$old" ] || continue
      curl -fsS -X DELETE "$API/images/$old" "${auth[@]}"
      echo "pruned old snapshot $old"
    done
echo "done: retention is $KEEP weekly snapshots"
