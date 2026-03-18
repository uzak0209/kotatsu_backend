#!/usr/bin/env bash
set -euo pipefail

# Required env vars:
#   CF_API_TOKEN   - Cloudflare API token with Zone DNS Edit permission
#   CF_ZONE_ID     - Target zone id
#   CF_RECORD_NAME - Record name (e.g. home.example.com)
# Optional env vars:
#   CF_RECORD_TYPE - A or AAAA (default: A)
#   CF_TTL         - TTL seconds, 1 means auto (default: 1)
#   CF_PROXIED     - true/false (default: false)
#   STATE_FILE     - Local last IP cache file
#   IP_CHECK_URL   - URL returning current public IP in plain text
#   DRY_RUN        - true/false

CF_RECORD_TYPE="${CF_RECORD_TYPE:-A}"
CF_TTL="${CF_TTL:-1}"
CF_PROXIED="${CF_PROXIED:-false}"
STATE_FILE="${STATE_FILE:-$HOME/.cache/cloudflare-ddns/last_${CF_RECORD_NAME:-record}_${CF_RECORD_TYPE}.txt}"
IP_CHECK_URL="${IP_CHECK_URL:-https://api.ipify.org}"
DRY_RUN="${DRY_RUN:-false}"

for key in CF_API_TOKEN CF_ZONE_ID CF_RECORD_NAME; do
  if [[ -z "${!key:-}" ]]; then
    echo "[error] missing required env var: $key" >&2
    exit 1
  fi
done

if [[ "$CF_RECORD_TYPE" != "A" && "$CF_RECORD_TYPE" != "AAAA" ]]; then
  echo "[error] CF_RECORD_TYPE must be A or AAAA" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "[error] curl not found" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "[error] jq not found" >&2
  exit 1
fi

mkdir -p "$(dirname "$STATE_FILE")"

get_public_ip() {
  curl -fsSL "$IP_CHECK_URL"
}

is_ipv4() {
  [[ "$1" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]
}

is_ipv6() {
  [[ "$1" == *:* ]]
}

PUBLIC_IP="$(get_public_ip | tr -d '[:space:]')"

if [[ "$CF_RECORD_TYPE" == "A" ]] && ! is_ipv4 "$PUBLIC_IP"; then
  echo "[error] IP_CHECK_URL did not return IPv4 address: $PUBLIC_IP" >&2
  exit 1
fi

if [[ "$CF_RECORD_TYPE" == "AAAA" ]] && ! is_ipv6 "$PUBLIC_IP"; then
  echo "[error] IP_CHECK_URL did not return IPv6 address: $PUBLIC_IP" >&2
  exit 1
fi

LAST_IP=""
if [[ -f "$STATE_FILE" ]]; then
  LAST_IP="$(cat "$STATE_FILE" 2>/dev/null || true)"
fi

if [[ "$PUBLIC_IP" == "$LAST_IP" ]]; then
  echo "[ok] unchanged ($CF_RECORD_TYPE): $PUBLIC_IP"
  exit 0
fi

if [[ "$DRY_RUN" == "true" ]]; then
  echo "[dry-run] detected new public IP for $CF_RECORD_NAME ($CF_RECORD_TYPE): ${LAST_IP:-<none>} -> $PUBLIC_IP"
  exit 0
fi

AUTH_HEADER="Authorization: Bearer $CF_API_TOKEN"
BASE_URL="https://api.cloudflare.com/client/v4"

RECORD_JSON="$(curl -fsSL -H "$AUTH_HEADER" \
  "$BASE_URL/zones/$CF_ZONE_ID/dns_records?type=$CF_RECORD_TYPE&name=$CF_RECORD_NAME")"

RECORD_ID="$(printf '%s' "$RECORD_JSON" | jq -r '.result[0].id // empty')"
CURRENT_DNS_IP="$(printf '%s' "$RECORD_JSON" | jq -r '.result[0].content // empty')"

if [[ -n "$CURRENT_DNS_IP" && "$CURRENT_DNS_IP" == "$PUBLIC_IP" ]]; then
  echo "$PUBLIC_IP" > "$STATE_FILE"
  echo "[ok] DNS already up to date ($CF_RECORD_TYPE): $PUBLIC_IP"
  exit 0
fi

PAYLOAD="$(jq -n \
  --arg type "$CF_RECORD_TYPE" \
  --arg name "$CF_RECORD_NAME" \
  --arg content "$PUBLIC_IP" \
  --argjson ttl "$CF_TTL" \
  --argjson proxied "$CF_PROXIED" \
  '{type:$type,name:$name,content:$content,ttl:$ttl,proxied:$proxied}')"

if [[ -n "$RECORD_ID" ]]; then
  UPDATE_RES="$(curl -fsSL -X PUT \
    -H "$AUTH_HEADER" \
    -H "Content-Type: application/json" \
    --data "$PAYLOAD" \
    "$BASE_URL/zones/$CF_ZONE_ID/dns_records/$RECORD_ID")"
  SUCCESS="$(printf '%s' "$UPDATE_RES" | jq -r '.success')"
  if [[ "$SUCCESS" != "true" ]]; then
    echo "[error] update failed: $UPDATE_RES" >&2
    exit 1
  fi
  echo "$PUBLIC_IP" > "$STATE_FILE"
  echo "[ok] updated $CF_RECORD_NAME ($CF_RECORD_TYPE): ${CURRENT_DNS_IP:-<none>} -> $PUBLIC_IP"
else
  CREATE_RES="$(curl -fsSL -X POST \
    -H "$AUTH_HEADER" \
    -H "Content-Type: application/json" \
    --data "$PAYLOAD" \
    "$BASE_URL/zones/$CF_ZONE_ID/dns_records")"
  SUCCESS="$(printf '%s' "$CREATE_RES" | jq -r '.success')"
  if [[ "$SUCCESS" != "true" ]]; then
    echo "[error] create failed: $CREATE_RES" >&2
    exit 1
  fi
  echo "$PUBLIC_IP" > "$STATE_FILE"
  echo "[ok] created $CF_RECORD_NAME ($CF_RECORD_TYPE): $PUBLIC_IP"
fi
