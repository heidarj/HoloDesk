#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: scripts/mac-remote-host-encode.sh <build|test|smoke> [branch] [-- <remote-script-args...>]" >&2
  exit 2
fi

if [[ -z "${HOLOBRIDGE_WINDOWS_HOST:-}" ]]; then
  echo "HOLOBRIDGE_WINDOWS_HOST is required" >&2
  exit 2
fi

if [[ -z "${HOLOBRIDGE_WINDOWS_USER:-}" ]]; then
  echo "HOLOBRIDGE_WINDOWS_USER is required" >&2
  exit 2
fi

action="$1"
shift

case "$action" in
  build) script_name="host-encode-build.ps1" ;;
  test) script_name="host-encode-test.ps1" ;;
  smoke) script_name="host-encode-smoke.ps1" ;;
  *)
    echo "unknown action: $action" >&2
    exit 2
    ;;
esac

branch=""
if [[ $# -gt 0 && "$1" != "--" ]]; then
  branch="$1"
  shift
fi

if [[ $# -gt 0 ]]; then
  if [[ "$1" != "--" ]]; then
    echo "expected '--' before remote script arguments" >&2
    exit 2
  fi
  shift
fi

if [[ -z "$branch" ]]; then
  branch="$(git rev-parse --abbrev-ref HEAD)"
fi

if [[ "$branch" == "HEAD" ]]; then
  echo "detached HEAD detected; pass an explicit branch name" >&2
  exit 2
fi

repo_path="${HOLOBRIDGE_WINDOWS_REPO_PATH:-C:\\Users\\${HOLOBRIDGE_WINDOWS_USER}\\source\\HoloDesk}"
ssh_target="${HOLOBRIDGE_WINDOWS_USER}@${HOLOBRIDGE_WINDOWS_HOST}"

ps_quote() {
  local value="$1"
  printf "'%s'" "${value//\'/\'\'}"
}

remote_args=""
for arg in "$@"; do
  remote_args+=" $(ps_quote "$arg")"
done

ssh "$ssh_target" powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command - <<EOF
\$ErrorActionPreference = 'Stop'
Set-Location $(ps_quote "$repo_path")
git fetch origin
git checkout $(ps_quote "$branch")
git pull --ff-only origin $(ps_quote "$branch")
& ".\\scripts\\$script_name"$remote_args
EOF
