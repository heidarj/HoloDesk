#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
host_dir="$repo_root/host"
client_package_dir="$repo_root/client-avp/HoloBridge/Packages/HoloBridgeClient"
artifact_dir="$repo_root/artifacts/client-mac-smoke"
server_log="$artifact_dir/quic_server.log"
private_key_path="/tmp/holobridge_test_priv.pem"
public_key_path="/tmp/holobridge_test_pub.pem"
user_store_path="$artifact_dir/authorized_users.json"
swiftpm_cache_root="/tmp/HoloBridgeSwiftPM"

mkdir -p "$artifact_dir"
mkdir -p "$swiftpm_cache_root/ModuleCache"

export SWIFTPM_MODULECACHE_OVERRIDE="$swiftpm_cache_root/ModuleCache"
export CLANG_MODULE_CACHE_PATH="$swiftpm_cache_root/ModuleCache"

pushd "$host_dir" >/dev/null
cargo build -p holobridge-transport --bin quic_server --bin test_keygen
popd >/dev/null

pushd "$client_package_dir" >/dev/null
swift build --product holobridge-client-smoke
smoke_bin="$(swift build --show-bin-path)/holobridge-client-smoke"
popd >/dev/null

export HOLOBRIDGE_AUTH_TEST_PRIVATE_KEY="$private_key_path"
export HOLOBRIDGE_AUTH_TEST_PUBLIC_KEY="$public_key_path"
"$host_dir/target/debug/test_keygen"

export HOLOBRIDGE_TRANSPORT_BIND="127.0.0.1"
export HOLOBRIDGE_TRANSPORT_PORT="4433"
export HOLOBRIDGE_AUTH_TEST_MODE="1"
export HOLOBRIDGE_AUTH_TEST_PUBLIC_KEY="$public_key_path"
export HOLOBRIDGE_AUTH_BUNDLE_ID="cloud.hr5.HoloBridge"
export HOLOBRIDGE_AUTH_USER_STORE="$user_store_path"
export HOLOBRIDGE_VIDEO_ENABLED="true"
export HOLOBRIDGE_VIDEO_SOURCE="synthetic"
export HOLOBRIDGE_VIDEO_SYNTHETIC_PRESET="transport-loopback-v1"
export HOLOBRIDGE_VIDEO_FRAME_RATE="60/1"
export RUST_LOG="${RUST_LOG:-info}"

server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

"$host_dir/target/debug/quic_server" >"$server_log" 2>&1 &
server_pid="$!"
sleep 1

if ! kill -0 "$server_pid" 2>/dev/null; then
  cat "$server_log" >&2
  echo "quic_server exited before the smoke client started" >&2
  exit 1
fi

"$smoke_bin" \
  --host 127.0.0.1 \
  --port 4433 \
  --allow-insecure-cert \
  --request-video \
  --resume-once \
  "$@"

echo "quic_server_log: $server_log"
