#!/usr/bin/env bash
set -euo pipefail

real_cc=${PERRY_ISSUE_8075_REAL_CC:-/usr/bin/cc}
runtime_library=${PERRY_ISSUE_8075_RUNTIME_LIBRARY:-}
stdlib_library=${PERRY_ISSUE_8075_STDLIB_LIBRARY:-}
is_shared=false
for argument in "$@"; do
  case "$argument" in
    -shared|-dynamiclib) is_shared=true ;;
  esac
done

if [[ "$is_shared" == true ]]; then
  [[ -f "$runtime_library" ]] || { echo "runtime provider is missing" >&2; exit 1; }
  [[ -f "$stdlib_library" ]] || { echo "stdlib provider is missing" >&2; exit 1; }
  if [[ $(uname -s) == Darwin ]]; then
    exec "$real_cc" "$@" \
      "$runtime_library" "$stdlib_library" \
      -Wl,-rpath,@loader_path -Wl,-dead_strip
  fi
  exec "$real_cc" "$@" \
    -Wl,--no-as-needed "$runtime_library" "$stdlib_library" -Wl,--as-needed \
    -Wl,--no-undefined -Wl,--gc-sections
fi

exec "$real_cc" "$@"
