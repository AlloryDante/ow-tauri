#!/bin/bash
# Runs W0c-A spike items on macOS in the invisible lab and proves, per run,
# that the app never became visible (window server: window-monitor,
# 10 ms polling) and never became the frontmost app (lsappinfo front).
#
#   ./run-mac.sh <out-dir> <spike-binary> [item ...]
#
# Items: ua gesture zoom close crash crash-hook storage (default: all).
set -u
OUT=${1:?out dir}
BIN=${2:?spike binary}
shift 2
ITEMS=${*:-ua gesture zoom close crash crash-hook storage}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
MON="$OUT/window-monitor"
if [ ! -x "$MON" ]; then
  swiftc -O -o "$MON" "$HERE/../../parity-harness/lib/window-monitor.swift" || exit 1
fi

front_pid() {
  local asn
  asn=$(lsappinfo front 2>/dev/null)
  [ -n "$asn" ] && lsappinfo info -only pid "$asn" 2>/dev/null | sed -n 's/.*"pid"=\([0-9]*\).*/\1/p'
}

for name in $ITEMS; do
  item=$name hook=0
  if [ "$name" = crash-hook ]; then item=crash hook=1; fi
  SPIKE_ITEM=$item SPIKE_HOOK=$hook SPIKE_OUT="$OUT/$name.json" "$BIN" >"$OUT/$name.log" 2>&1 &
  pid=$!
  "$MON" "$pid" "$OUT/$name.windows.jsonl" 10 &
  mon=$!
  if [ -n "${LLDB:-}" ]; then
    lldb -p "$pid" --batch -o continue -k "bt all" -k "kill" >"$OUT/$name.lldb.txt" 2>&1 &
  fi
  front=0 samples=0 t=0
  while kill -0 "$pid" 2>/dev/null; do
    samples=$((samples + 1))
    [ "$(front_pid)" = "$pid" ] && front=$((front + 1))
    sleep 0.2
    t=$((t + 1))
    if [ $t -gt 900 ]; then echo "timeout: $name"; kill "$pid"; fi
  done
  wait "$mon" 2>/dev/null
  echo "{\"name\":\"$name\",\"frontSamples\":$samples,\"appFrontSamples\":$front,\"everFront\":$([ $front -gt 0 ] && echo true || echo false)}" >"$OUT/$name.front.json"
  echo "$name: $(tail -1 "$OUT/$name.windows.jsonl") $(cat "$OUT/$name.front.json")"
  grep -h "spike:" "$OUT/$name.log"
done
