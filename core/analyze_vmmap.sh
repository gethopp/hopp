#!/usr/bin/env bash
# analyze_vmmap.sh - Extract and compare relevant vmmap metrics for IOSurface leak investigation
# Usage: ./analyze_vmmap.sh first_call.txt [second_call.txt]

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <vmmap_file_1> [vmmap_file_2]"
  exit 1
fi

FILE1="$1"
FILE2="${2:-}"

extract_summary() {
  local file="$1"
  local label="$2"
  
  echo "=== $label: $(basename "$file") ==="
  
  # Physical footprint
  echo "--- Physical Footprint ---"
  grep "Physical footprint" "$file" | sed 's/^/  /'
  
  # REGION TYPE summary (usually at the end of vmmap output)
  echo ""
  echo "--- Key Region Types ---"
  # Extract the summary table (lines with REGION TYPE, TOTAL, and key memory regions)
  awk '
    /^REGION TYPE/ { in_summary=1; next }
    in_summary && /^=+/ { in_summary=0 }
    in_summary && /(IOSurface|MALLOC|Stack|TOTAL|IOAccelerator|CoreAnimation)/ {
      # Print matching lines with indentation
      print "  " $0
    }
  ' "$file"
  
  # Individual large IOSurface entries (>1MB)
  echo ""
  echo "--- Large IOSurface Entries (>1MB) ---"
  grep "IOSurface" "$file" | grep -E "\[ *[0-9]+\.?[0-9]*M" | sed 's/^/  /'
  
  echo ""
}

extract_summary "$FILE1" "First Call"

if [[ -n "$FILE2" ]]; then
  extract_summary "$FILE2" "Second Call"
  
  echo "=== COMPARISON ==="
  echo ""
  echo "--- Physical Footprint Diff ---"
  pf1=$(grep "Physical footprint:" "$FILE1" | grep -oP '[\d.]+M' | head -1)
  pf2=$(grep "Physical footprint:" "$FILE2" | grep -oP '[\d.]+M' | head -1)
  echo "  $FILE1: $pf1"
  echo "  $FILE2: $pf2"
  
  echo ""
  echo "--- IOSurface Count ---"
  count1=$(grep -c "IOSurface" "$FILE1" || true)
  count2=$(grep -c "IOSurface" "$FILE2" || true)
  echo "  $FILE1: $count1 surfaces"
  echo "  $FILE2: $count2 surfaces"
  echo "  Delta: $((count2 - count1))"
  
  echo ""
  echo "--- New IOSurfaces in Second Call ---"
  # Extract SurfaceIDs from both files
  grep "IOSurface" "$FILE1" | grep -oP "SurfaceID: 0x[0-9a-f]+" | sort -u > /tmp/surfaces1.txt || true
  grep "IOSurface" "$FILE2" | grep -oP "SurfaceID: 0x[0-9a-f]+" | sort -u > /tmp/surfaces2.txt || true
  
  # Find surfaces only in file2
  comm -13 /tmp/surfaces1.txt /tmp/surfaces2.txt | while read -r sid; do
    grep "$sid" "$FILE2" | sed 's/^/  /'
  done
  
  echo ""
  echo "--- Persistent IOSurfaces (shared with WindowServer) ---"
  grep "shared with WindowServer" "$FILE2" | sed 's/^/  /' || echo "  None found"
  
  echo ""
  rm -f /tmp/surfaces1.txt /tmp/surfaces2.txt
fi
