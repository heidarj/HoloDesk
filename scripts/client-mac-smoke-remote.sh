#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
client_package_dir="$repo_root/client-avp/HoloBridge/Packages/HoloBridgeClient"
swiftpm_cache_root="/tmp/HoloBridgeSwiftPM"

mkdir -p "$swiftpm_cache_root/ModuleCache"

export SWIFTPM_MODULECACHE_OVERRIDE="$swiftpm_cache_root/ModuleCache"
export CLANG_MODULE_CACHE_PATH="$swiftpm_cache_root/ModuleCache"

pushd "$client_package_dir" >/dev/null
swift build --product holobridge-client-smoke
smoke_bin="$(swift build --show-bin-path)/holobridge-client-smoke"
popd >/dev/null

exec "$smoke_bin" "$@"
