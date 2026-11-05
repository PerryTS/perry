#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=${MBX_BLAZE_ROOT:-"$script_dir/../.blaze"}
revision=$(cat "$script_dir/build-tools.ref")
case "$revision" in
  *[!0-9a-f]*|'') printf '%s\n' 'Invalid build-tools revision' >&2; exit 1 ;;
esac
if [ "${#revision}" -ne 40 ]; then
  printf '%s\n' 'Build tools must name an immutable Git commit' >&2
  exit 1
fi

bootstrap="$root/bootstrap/$revision"
mkdir -p "$bootstrap"
if [ ! -f "$bootstrap/install.py" ] || [ ! -f "$bootstrap/lock.json" ]; then
  temporary=$(mktemp -d "$bootstrap/download.XXXXXX")
  trap 'rm -rf "$temporary"' EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  for file in install.py lock.json; do
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 \
      "https://raw.githubusercontent.com/jdalton/blaze/$revision/fleet/toolchain/$file" \
      -o "$temporary/$file"
  done
  mv "$temporary/install.py" "$bootstrap/install.py"
  mv "$temporary/lock.json" "$bootstrap/lock.json"
  rmdir "$temporary"
  trap - EXIT INT TERM
fi
if [ "${1:-}" = "--github-env" ]; then
  python3 "$bootstrap/install.py" --lock "$bootstrap/lock.json" --root "$root" --github-env
  exit
fi
python3 "$bootstrap/install.py" --lock "$bootstrap/lock.json" --root "$root"
if [ "$#" -gt 0 ]; then
  . "$root/env.sh"
  exec "$@"
fi
