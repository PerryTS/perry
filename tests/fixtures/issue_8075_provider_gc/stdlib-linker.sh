#!/usr/bin/env bash
set -euo pipefail

runtime_library=${PERRY_ISSUE_8075_RUNTIME_LIBRARY:?set the runtime provider path}
real_cc=${PERRY_ISSUE_8075_REAL_CC:-/usr/bin/cc}
host_os=$(uname -s)
arguments=()
skip_export_list_value=false
original_export_list=""
original_version_script=""
saw_runtime_rlib=false
stdlib_provider_exports=(
  issue_8075_stdlib_runtime_probe
  js_fetch_response_status
  js_fetch_response_status_text
  js_headers_append
  js_headers_get
  js_headers_new
  js_headers_set
  js_readable_stream_get_reader_with_options
  js_readable_stream_new_from_source_object
  js_reader_read
  js_response_body
  js_response_body_init_ptr
  js_response_get_headers
  js_response_new
  js_stdlib_init_dispatch
  js_stream_unwrap_handle
)

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
    -Wl,--version-script=*)
      original_version_script=${argument#-Wl,--version-script=}
      ;;
    *) arguments+=("$argument") ;;
  esac
done

custom_export_list=""
custom_version_script=""
cleanup() {
  [[ -z "$custom_export_list" ]] || rm -f "$custom_export_list"
  [[ -z "$custom_version_script" ]] || rm -f "$custom_version_script"
}
trap cleanup EXIT

if [[ -n "$original_export_list" ]]; then
  if [[ "$saw_runtime_rlib" == true ]]; then
    custom_export_list=$(mktemp "${TMPDIR:-/tmp}/perry-8075-exports.XXXXXX")
    {
      sed -n '/issue_8075_stdlib_runtime_probe/p' "$original_export_list"
      printf '_%s\n' "${stdlib_provider_exports[@]}"
      nm -gU "$runtime_library" | awk 'NF >= 3 { print $3 }'
    } | sort -u > "$custom_export_list"
    arguments+=('-Wl,-exported_symbols_list' "-Wl,$custom_export_list")
  else
    arguments+=('-Wl,-exported_symbols_list' "-Wl,$original_export_list")
  fi
fi

if [[ -n "$original_version_script" ]]; then
  if [[ "$saw_runtime_rlib" == true ]]; then
    custom_version_script=$(mktemp "${TMPDIR:-/tmp}/perry-8075-version.XXXXXX")
    {
      echo '{ global:'
      printf '  %s;\n' "${stdlib_provider_exports[@]}"
      # The runtime's own symbols must stay GLOBAL here, exactly as the
      # Mach-O branch above re-exports them.
      #
      # This provider statically links the runtime rlib as well as loading the
      # runtime .so, so it carries its own definition of `js_gc_init` and
      # friends. `local: *` binds those internally and makes them
      # non-preemptible, so the stdlib stops resolving stateful runtime calls
      # to the image the host loaded first — which is the exact thing this
      # fixture exists to detect, and it reported it as
      # "stdlib provider is bound to a different runtime image" (#8089).
      # `-D`: the DYNAMIC symbol table. This branch is ELF-only, and what
      # governs preemption is what the dynamic linker sees, not the static
      # symtab a stripped .so need not carry at all. (The Mach-O branch's
      # plain `nm -gU` is right for its format, where the dylib's table is
      # what plain nm reads.)
      nm -D --defined-only "$runtime_library" | awk 'NF >= 3 { printf "  %s;\n", $3 }'
      echo 'local: *; };'
    } > "$custom_version_script"
    arguments+=("-Wl,--version-script=$custom_version_script")
  else
    arguments+=("-Wl,--version-script=$original_version_script")
  fi
fi

if [[ "$saw_runtime_rlib" == true && "$host_os" == Darwin ]]; then
  arguments+=('-Wl,-rpath,@loader_path' '-Wl,-flat_namespace' '-Wl,-interposable')
elif [[ "$saw_runtime_rlib" == true ]]; then
  # shellcheck disable=SC2016 # $ORIGIN must reach the ELF linker literally.
  arguments+=('-Wl,-rpath,$ORIGIN' '-Wl,-soname,libperry_stdlib.so')
fi

"$real_cc" "${arguments[@]}"
