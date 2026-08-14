#!/usr/bin/env bash
set -euo pipefail

runtime_library=${PERRY_NEXT_RUNTIME_LIBRARY:?set the runtime provider path}
required_symbols=${PERRY_NEXT_REQUIRED_SYMBOLS:?set the app undefined-symbol list}
real_cc=${PERRY_NEXT_REAL_CC:-/usr/bin/cc}
host_os=$(uname -s)
original_arguments=("$@")
arguments=()
rlibs=()
original_export_list=""
skip_export_list_value=false
saw_runtime_rlib=false
is_runtime_dylib=false

for argument in "$@"; do
    if [[ "$skip_export_list_value" == true ]]; then
        original_export_list=${argument#-Wl,}
        skip_export_list_value=false
        continue
    fi
    case "$argument" in
        */libperry_runtime.dylib)
            is_runtime_dylib=true
            arguments+=("$argument")
            ;;
        */libperry_runtime.rlib|*libperry_runtime-*.rlib)
            saw_runtime_rlib=true
            if [[ "$host_os" == Linux ]]; then
                arguments+=(
                    '-Wl,-Bdynamic' '-Wl,--no-as-needed' "$runtime_library"
                    '-Wl,--as-needed' '-Wl,-Bstatic'
                )
            else
                arguments+=("$runtime_library")
            fi
            ;;
        *.rlib)
            rlibs+=("$argument")
            arguments+=("$argument")
            ;;
        -Wl,-exported_symbols_list)
            skip_export_list_value=true
            ;;
        -Wl,-exported_symbols_list,*)
            original_export_list=${argument#-Wl,-exported_symbols_list,}
            ;;
        -Wl,--version-script=*)
            # Replaced below with a script that also exports selected dependency ABI.
            ;;
        *) arguments+=("$argument") ;;
    esac
done

if [[ "$saw_runtime_rlib" != true ]]; then
    if [[ "$host_os" == Darwin && "$is_runtime_dylib" == true ]]; then
        exec "$real_cc" "${original_arguments[@]}" \
            -framework CoreFoundation -framework Foundation
    fi
    exec "$real_cc" "${original_arguments[@]}"
fi

if [[ "$host_os" == Darwin ]]; then
    # The provider records this install name while linking. The gate copies
    # both images beside one another before loading them.
    install_name_tool -id '@rpath/libperry_runtime.dylib' "$runtime_library"
fi

scratch=$(mktemp -d "${TMPDIR:-/tmp}/perry-next-provider-link.XXXXXX")
cleanup() {
    rm -rf "$scratch"
}
trap cleanup EXIT

defined="$scratch/defined"
selected="$scratch/selected"
: >"$defined"
for rlib in "${rlibs[@]}"; do
    if [[ "$host_os" == Darwin ]]; then
        nm -gU "$rlib" 2>/dev/null | awk 'NF >= 3 { print $3 }' >>"$defined" || true
    else
        nm -g --defined-only "$rlib" 2>/dev/null | awk 'NF >= 3 { print $3 }' >>"$defined" || true
    fi
done
sort -u "$defined" -o "$defined"
sort -u "$required_symbols" | comm -12 - "$defined" >"$selected"

if [[ ! -s "$selected" ]]; then
    echo "provider link selected no app ABI symbols" >&2
    exit 1
fi

if [[ "$host_os" == Darwin ]]; then
    exports="$scratch/exports"
    {
        if [[ -n "$original_export_list" ]]; then
            sed -n '/next_app_route_provider_runtime_probe/p' "$original_export_list"
        else
            echo '_next_app_route_provider_runtime_probe'
        fi
        cat "$selected"
    } | sort -u >"$exports"
    while IFS= read -r symbol; do
        arguments+=("-Wl,-u,$symbol")
    done <"$selected"
    arguments+=(
        '-Wl,-exported_symbols_list' "-Wl,$exports"
        '-Wl,-rpath,@loader_path' '-Wl,-flat_namespace' '-Wl,-interposable'
    )
else
    version_script="$scratch/exports.map"
    {
        echo '{ global:'
        echo 'next_app_route_provider_runtime_probe;'
        sed 's/^/  /; s/$/;/' "$selected"
        echo 'local: *; };'
    } >"$version_script"
    while IFS= read -r symbol; do
        arguments+=("-Wl,--undefined=$symbol")
    done <"$selected"
    arguments+=("-Wl,--version-script=$version_script")
    # shellcheck disable=SC2016 # $ORIGIN must reach the ELF linker literally.
    arguments+=('-Wl,-rpath,$ORIGIN' '-Wl,-soname,libperry_stdlib.so')
fi

"$real_cc" "${arguments[@]}"
