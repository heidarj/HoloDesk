#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
host_dir="$repo_root/host"
client_package_dir="$repo_root/client-avp/HoloBridge/Packages/HoloBridgeClient"
artifact_dir="$repo_root/artifacts/client-mac-quic-interop"
swiftpm_cache_root="/tmp/HoloBridgeSwiftPM"
swiftpm_scratch_root="/tmp/HoloBridgeClientInteropBuild"

mkdir -p "$artifact_dir"
mkdir -p "$swiftpm_cache_root/ModuleCache"

export SWIFTPM_MODULECACHE_OVERRIDE="$swiftpm_cache_root/ModuleCache"
export CLANG_MODULE_CACHE_PATH="$swiftpm_cache_root/ModuleCache"
export HOLOBRIDGE_TRANSPORT_BIND="${HOLOBRIDGE_TRANSPORT_BIND:-127.0.0.1}"
export HOLOBRIDGE_TRANSPORT_PORT="${HOLOBRIDGE_TRANSPORT_PORT:-4433}"
export RUST_LOG="${RUST_LOG:-info}"

selected_mode="all"
client_args=()

while (($#)); do
  case "$1" in
    --mode)
      if (($# < 2)); then
        echo "--mode requires a value" >&2
        exit 1
      fi
      selected_mode="$2"
      shift 2
      ;;
    *)
      client_args+=("$1")
      shift
      ;;
  esac
done

case "$selected_mode" in
  all)
    modes=(stream datagram mixed)
    ;;
  stream|datagram|mixed)
    modes=("$selected_mode")
    ;;
  *)
    echo "unsupported mode: $selected_mode" >&2
    exit 1
    ;;
esac

pushd "$host_dir" >/dev/null
cargo build -p holobridge-transport --bin quic_interop_server
host_bin="$host_dir/target/debug/quic_interop_server"
popd >/dev/null

pushd "$client_package_dir" >/dev/null
swift build --scratch-path "$swiftpm_scratch_root" --product holobridge-quic-interop-smoke
client_bin="$(swift build --scratch-path "$swiftpm_scratch_root" --show-bin-path)/holobridge-quic-interop-smoke"
popd >/dev/null

server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

for mode in "${modes[@]}"; do
  log_path="$artifact_dir/$mode.server.log"
  rm -f "$log_path"

  echo "=== quic interop mode: $mode ==="
  "$host_bin" --mode "$mode" >"$log_path" 2>&1 &
  server_pid="$!"
  sleep 1

  if ! kill -0 "$server_pid" 2>/dev/null; then
    cat "$log_path" >&2
    echo "quic_interop_server exited before the client started" >&2
    exit 1
  fi

  set +e
  client_command=(
    "$client_bin"
    --mode "$mode"
    --host "$HOLOBRIDGE_TRANSPORT_BIND"
    --port "$HOLOBRIDGE_TRANSPORT_PORT"
    --allow-insecure-cert
  )
  if ((${#client_args[@]})); then
    client_command+=("${client_args[@]}")
  fi
  "${client_command[@]}"
  client_status=$?
  set -e

  wait "$server_pid"
  server_status=$?
  server_pid=""

  if ((client_status != 0 || server_status != 0)); then
    echo "--- server log: $log_path ---" >&2
    cat "$log_path" >&2
    echo "interop mode '$mode' failed (client=$client_status server=$server_status)" >&2
    exit 1
  fi

  echo "server_log: $log_path"
done
