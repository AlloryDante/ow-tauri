#!/usr/bin/env bash
# W0c-B7: can a plugin crate turn on tauri's `unstable` feature on Windows and
# macOS only (DESIGN-v2 §4.6)? Nothing is compiled:
#   * `cargo tree -e features -i tauri` (feature resolution), and
#   * `cargo check --unit-graph` (Cargo's build plan, which is where Cargo
#     rejects a crate that depends on one package under two names; needs
#     RUSTC_BOOTSTRAP=1 for the -Z flag on a stable toolchain).
# Variants: v1 = the design's renamed `tauri-unstable` dependency and v2 = the
# same `tauri` name in a target table (both in b7-variants/); v3 = a separate
# shim crate (miniplugin + unstable-shim here, also what the spike app builds
# on Windows CI). One VERDICT line per case.
set -u
cd "$(dirname "$0")"
fail=0
case_() { # dir, target, expect-unstable(0|1), cargo args...
  local dir=$1 target=$2 expect=$3; shift 3
  local tree n got=0 graph units verdict=ok
  tree=$(cd "$dir" && cargo tree -e features -i tauri --target "$target" "$@" 2>&1)
  n=$(grep -c 'tauri feature "unstable"' <<<"$tree")
  [ "$n" -gt 0 ] && got=1
  graph=$(cd "$dir" && RUSTC_BOOTSTRAP=1 cargo check --unit-graph -Z unstable-options --target "$target" "$@" 2>&1 >/dev/null | grep -m1 '^error' || true)
  [ "$got" = "$expect" ] || verdict=FAIL
  [ -z "$graph" ] || verdict=FAIL
  [ "$verdict" = ok ] || fail=1
  echo "VERDICT $verdict dir=$dir target=$target args=[$*] unstable=$got expected=$expect unit-graph=${graph:-ok}"
  echo "$tree" | sed 's/^/    /'
}
matrix() { # dir, app-package, plugin-package
  local d=$1 app=$2 plugin=$3
  for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-linux-android; do
    case_ "$d" "$t" 0 -p "$app"
    case_ "$d" "$t" 0 -p "$plugin"
  done
  for t in x86_64-pc-windows-msvc aarch64-pc-windows-msvc x86_64-apple-darwin aarch64-apple-darwin; do
    case_ "$d" "$t" 1 -p "$app"
    case_ "$d" "$t" 1 -p "$plugin"
    case_ "$d" "$t" 0 -p "$plugin" --no-default-features --features plugin
    case_ "$d" "$t" 0 -p "$plugin" --no-default-features --features build
  done
}
matrix b7-variants v1-app v1-plugin
matrix b7-variants v2-app v2-plugin
matrix . win-mechanics-spike spike-miniplugin
exit $fail
