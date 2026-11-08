#!/usr/bin/env bash
# Opt-in macOS LiveKit shaping. Each session owns its own anchor, pipes and PF reference.
# sudo ./netem.sh bad --remote 127.0.0.1 --session run123 [--port 7880]
# Server ports must be signaling PORT, ICE/TCP PORT+1, ICE/UDP PORT+2 (LiveKit dev defaults).
# sudo ./netem.sh off --session run123
# ./netem.sh --self-test
set -euo pipefail

settings() {
  case "$1" in
    good) echo '20Mbit/s 50Mbit/s 10 0' ;;
    medium) echo '3Mbit/s 8Mbit/s 40 0.005' ;;
    bad) echo '1Mbit/s 2Mbit/s 75 0.02' ;;
    3g) echo '750Kbit/s 1600Kbit/s 150 0.01' ;;
    drop) echo '0 0 0 1' ;;
    *) echo "Unknown network profile: $1" >&2; return 1 ;;
  esac
}

rules() {
  local host="$1" port="$2" iface="$3" up="$4" down="$5"
  local ports="{ $port $((port + 1)) $((port + 2)) }"
  if [ "$iface" = lo0 ]; then
    echo "dummynet out on lo0 proto { tcp udp } from any to $host port $ports pipe $up"
    echo "dummynet out on lo0 proto { tcp udp } from $host port $ports to any pipe $down"
  else
    echo "dummynet out on $iface proto { tcp udp } from any to $host port $ports pipe $up"
    echo "dummynet in on $iface proto { tcp udp } from $host port $ports to any pipe $down"
  fi
}

if [ "${1:-}" = --self-test ]; then
  for profile in good medium bad 3g drop; do
    read -r up down delay loss <<< "$(settings "$profile")"
    [ -n "$up" ] && [ -n "$down" ] && [[ "$delay" =~ ^[0-9]+$ ]] && [ -n "$loss" ]
  done
  ! settings invalid >/dev/null 2>&1
  local_rules="$(rules 127.0.0.1 7880 lo0 41001 41002)"
  [[ "$local_rules" == *'to 127.0.0.1 port { 7880 7881 7882 } pipe 41001'* ]]
  [[ "$local_rules" == *'from 127.0.0.1 port { 7880 7881 7882 } to any pipe 41002'* ]]
  [[ "$(rules 192.0.2.1 9000 en0 41001 41002)" == *'dummynet in on en0'* ]]
  echo 'PASS network profile and directional-rule checks'
  exit 0
fi

profile="${1:-}"
[ -n "$profile" ] || { echo 'Usage: netem.sh PROFILE|off --session TOKEN [--remote HOST] [--port PORT]' >&2; exit 1; }
shift
session="" host=127.0.0.1 port=7880
while [ $# -gt 0 ]; do
  [ $# -ge 2 ] || { echo "Missing value for $1" >&2; exit 1; }
  case "$1" in
    --session) session="$2" ;;
    --remote) host="$2" ;;
    --port) port="$2" ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
  shift 2
done
[[ "$session" =~ ^[a-zA-Z0-9_-]{1,48}$ ]] || { echo 'A unique --session token is required' >&2; exit 1; }
[[ "$host" =~ ^[a-zA-Z0-9.:_-]+$ ]] || { echo 'Invalid --remote host' >&2; exit 1; }
[[ "$port" =~ ^[0-9]{1,5}$ ]] && [ "$port" -gt 0 ] && [ "$port" -le 65533 ] || { echo 'Invalid LiveKit port' >&2; exit 1; }
port=$((10#$port))
[ "$(uname -s)" = Darwin ] || { echo 'Network shaping requires macOS' >&2; exit 1; }
[ "$(id -u)" -eq 0 ] || { echo 'Network shaping requires sudo; first run sudo -v' >&2; exit 1; }

anchor="com.apple/hopp-smoke-$session"
state="/var/run/hopp-smoke-netem-$session"
token="" up_pipe="" down_pipe=""
if [ -f "$state" ]; then
  read -r token up_pipe down_pipe < "$state"
fi
[ "$token" != - ] || token=""

release() {
  local failed=0
  pfctl -q -a "$anchor" -F all || failed=1
  for pipe in "$up_pipe" "$down_pipe"; do
    if [ -n "$pipe" ] && dnctl pipe show "$pipe" 2>/dev/null | /usr/bin/grep -q .; then
      dnctl -q pipe delete "$pipe" || failed=1
    fi
  done
  if [ -n "$token" ]; then pfctl -q -X "$token" || failed=1; fi
  if [ "$failed" -eq 0 ]; then rm -f "$state"; fi
  return "$failed"
}

if [ "$profile" = off ]; then
  if [ -f "$state" ]; then release; fi
  echo "netem: released $session"
  exit 0
fi
profile_values="$(settings "$profile")"
read -r up down delay loss <<< "$profile_values"

# Reserve a pair without replacing pipes belonging to another application/session.
if [ -z "$up_pipe" ]; then
  base=$((41000 + ($(printf '%s' "$session" | cksum | awk '{print $1}') % 10000) * 2))
  up_pipe="$base" down_pipe="$((base + 1))"
  for pipe in "$up_pipe" "$down_pipe"; do
    if dnctl pipe show "$pipe" 2>/dev/null | /usr/bin/grep -q .; then
      echo "Pipe $pipe already exists; use another session token" >&2
      exit 1
    fi
  done
fi

trap 'release || true' ERR
trap 'release || true; exit 130' INT TERM
umask 077
# Persist ownership before applying, so the runner can clean up interrupted setup.
printf '%s %s %s\n' "${token:--}" "$up_pipe" "$down_pipe" > "$state"
[ "$token" != - ] || token=""
dnctl pipe "$up_pipe" config bw "$up" delay "$delay" plr "$loss"
dnctl pipe "$down_pipe" config bw "$down" delay "$delay" plr "$loss"
case "$host" in
  localhost) host=127.0.0.1; iface=lo0 ;;
  127.*|::1) iface=lo0 ;;
  *) iface="$(route -n get "$host" | awk '/interface:/ {print $2; exit}')" ;;
esac
[[ "$iface" =~ ^[a-zA-Z0-9]+$ ]] || { echo 'Cannot find network interface' >&2; false; }
rules "$host" "$port" "$iface" "$up_pipe" "$down_pipe" | pfctl -q -a "$anchor" -f -
if [ -z "$token" ]; then
  token="$(pfctl -E 2>&1 | awk '/Token/ {print $NF}')"
  [[ "$token" =~ ^[0-9]+$ ]] || { echo 'Could not acquire PF reference' >&2; false; }
fi
printf '%s %s %s\n' "$token" "$up_pipe" "$down_pipe" > "$state"
trap - ERR INT TERM
echo "netem: $profile for $host:$port (session $session)"
