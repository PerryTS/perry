#!/usr/bin/env bash
set -euo pipefail

runtime_library=${PERRY_ISSUE_8075_RUNTIME_LIBRARY:?set the runtime provider path}
real_cc=${PERRY_ISSUE_8075_REAL_CC:-/usr/bin/cc}
host_os=$(uname -s)
arguments=()
skip_export_list_value=false
original_export_list=""
saw_runtime_rlib=false

for argument in "$@"; do
  if [[ "$skip_export_list_value" == true ]]; then
    original_export_list=${argument#-Wl,}
    skip_export_list_value=false
    continue
  fi
  case "$argument" in
    *libperry_runtime-*.rlib)
      saw_runtime_rlib=true
      if [[ "$host_os" == Linux ]]; then
        arguments+=(
          '-Wl,-Bdynamic' '-Wl,--no-as-needed' "$runtime_library"
          '-Wl,--as-needed' '-Wl,-Bstatic' "$argument"
        )
      else
        arguments+=("$runtime_library" "$argument")
      fi
      ;;
    -Wl,-exported_symbols_list)
      skip_export_list_value=true
      ;;
    -Wl,-exported_symbols_list,*)
      original_export_list=${argument#-Wl,-exported_symbols_list,}
      ;;
    *) arguments+=("$argument") ;;
  esac
done

custom_export_list=""
cleanup() {
  [[ -z "$custom_export_list" ]] || rm -f "$custom_export_list"
}
trap cleanup EXIT

if [[ -n "$original_export_list" ]]; then
  if [[ "$saw_runtime_rlib" == true ]]; then
    custom_export_list=$(mktemp "${TMPDIR:-/tmp}/perry-8075-exports.XXXXXX")
    {
      sed -n '/issue_8075_stdlib_runtime_probe/p' "$original_export_list"
      nm -gU "$runtime_library" | awk 'NF >= 3 { print $3 }'
    } | sort -u > "$custom_export_list"
    arguments+=('-Wl,-exported_symbols_list' "-Wl,$custom_export_list")
  else
    arguments+=('-Wl,-exported_symbols_list' "-Wl,$original_export_list")
  fi
fi

if [[ "$saw_runtime_rlib" == true && "$host_os" == Darwin ]]; then
  arguments+=('-Wl,-rpath,@loader_path' '-Wl,-flat_namespace' '-Wl,-interposable')
elif [[ "$saw_runtime_rlib" == true ]]; then
  # shellcheck disable=SC2016 # $ORIGIN must reach the ELF linker literally.
  arguments+=('-Wl,-rpath,$ORIGIN' '-Wl,-soname,libperry_stdlib.so')
fi

"$real_cc" "${arguments[@]}"
