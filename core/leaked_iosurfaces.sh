#!/usr/bin/env bash
# leaked_iosurfaces.sh - Print leaked IOSurfaces from vmmap output.
#
# A "leaked" IOSurface is one with a SurfaceID present in the END capture but
# not in the BASELINE capture (i.e. allocated during the call and never freed).
#
# Usage:
#   ./leaked_iosurfaces.sh <end.txt> [baseline.txt]
#
# With only <end.txt>: lists every IOSurface that has a SurfaceID (real GPU
# surfaces; the anonymous 16K SHM entries are skipped).
# With [baseline.txt]: lists only the SurfaceIDs that are new in <end.txt>.

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <end.txt> [baseline.txt]" >&2
  exit 1
fi

END_FILE="$1"
BASELINE_FILE="${2:-}"

# Emit "SurfaceID<TAB>DIRTY<TAB>dims<TAB>label" for each IOSurface region that
# carries a SurfaceID. DIRTY is the 3rd value inside the [ ... ] bracket.
extract() {
  # Match real GPU IOSurfaces by their WxH dimensions. The SurfaceID label is
  # sometimes truncated by vmmap to "...xNNN" when the detail is long (e.g.
  # "shared with WindowServer"), so we read the hex id from the token right
  # before the dimensions instead of relying on the literal "SurfaceID:".
  awk '
    /^IOSurface/ && match($0, /[0-9][0-9][0-9]+x[0-9][0-9][0-9]+/) {
      dims = substr($0, RSTART, RLENGTH)

      # DIRTY size = 3rd token between [ and ]
      b = substr($0, index($0, "[") + 1)
      b = substr(b, 1, index(b, "]") - 1)
      n = split(b, sz, /[[:space:]]+/)
      dirty = ""
      c = 0
      for (i = 1; i <= n; i++) if (sz[i] != "") { c++; if (c == 3) dirty = sz[i] }

      # id = field just before the dims field; normalize "0x320"/"...x34f" -> "0x320"/"0x34f"
      id = ""
      for (i = 1; i <= NF; i++) if ($i == dims) { id = $(i-1); break }
      sub(/^(0x|\.\.\.x|\.\.\.)/, "", id)
      if (id != "") id = "0x" id

      label = ""
      if (match($0, /'\''[^'\'']*'\''/)) label = substr($0, RSTART, RLENGTH)

      printf "%s\t%s\t%s\t%s\n", id, dirty, dims, label
    }
  ' "$1"
}

# Convert a vmmap size token (e.g. 23.0M, 16K, 512K, 1.5G) to bytes.
to_bytes() {
  awk -v s="$1" 'BEGIN {
    u = substr(s, length(s), 1)
    v = substr(s, 1, length(s) - 1)
    if (u == "K") print v * 1024
    else if (u == "M") print v * 1024 * 1024
    else if (u == "G") print v * 1024 * 1024 * 1024
    else print s + 0
  }'
}

human() {
  awk -v b="$1" 'BEGIN {
    if (b >= 1024*1024*1024) printf "%.1fG\n", b/1024/1024/1024
    else if (b >= 1024*1024) printf "%.1fM\n", b/1024/1024
    else if (b >= 1024) printf "%.1fK\n", b/1024
    else printf "%dB\n", b
  }'
}

if [[ -n "$BASELINE_FILE" ]]; then
  baseline_ids="$(extract "$BASELINE_FILE" | cut -f1 | sort -u)"
  echo "=== Leaked IOSurfaces (in $END_FILE, not in $BASELINE_FILE) ==="
else
  baseline_ids=""
  echo "=== IOSurfaces with SurfaceID in $END_FILE ==="
fi

total=0
count=0
while IFS=$'\t' read -r id dirty dims label; do
  [[ -z "$id" ]] && continue
  if [[ -n "$baseline_ids" ]] && grep -qx "$id" <<<"$baseline_ids"; then
    continue
  fi
  printf "  %-8s  %-8s  %-12s  %s\n" "$id" "$dirty" "$dims" "$label"
  total=$((total + $(to_bytes "$dirty")))
  count=$((count + 1))
done < <(extract "$END_FILE")

echo "---"
echo "  leaked surfaces: $count"
echo "  leaked dirty total: $(human "$total")"
