#!/bin/bash
# Runs the spike on macOS in the invisible lab and proves the app never
# became visible (window server: alpha 0 / never on a display) and never
# became the frontmost app (Launch Services).
#
#   ./run-mac.sh <out-dir> [stable-bin] [unstable-bin]
#
# Build first (separate target dirs so the two feature sets do not rebuild
# each other):
#   CARGO_TARGET_DIR=<t>/stable   cargo build
#   CARGO_TARGET_DIR=<t>/unstable cargo build --features unstable
set -u
OUT=${1:?out dir}
STABLE=${2:-target/stable/debug/unstable-input-spike}
UNSTABLE=${3:-target/unstable/debug/unstable-input-spike}
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

run_one() { # name bin mode delivery fakekey
  local name=$1 bin=$2 mode=$3 delivery=$4 fake=$5
  SPIKE_MODE=$mode SPIKE_DELIVERY=$delivery SPIKE_FAKE_KEY=$fake SPIKE_OUT="$OUT/$name.json" \
    "$bin" >"$OUT/$name.log" 2>&1 &
  local pid=$!
  "$MON" "$pid" "$OUT/$name.windows.jsonl" 10 &
  local mon=$!
  local front=0 samples=0 t=0
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
}

DELIVERY=${SPIKE_DELIVERY:-app}
FAKE=${SPIKE_FAKE_KEY:-1}
ONLY=${ONLY:-stable unstable-nochild unstable-child}
for m in $ONLY; do
  if [ "$m" = stable ]; then bin=$STABLE; else bin=$UNSTABLE; fi
  run_one "$m-$DELIVERY" "$bin" "$m" "$DELIVERY" "$FAKE"
done
