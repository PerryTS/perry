#!/usr/bin/env bash
set -euo pipefail

# #8075/#8038: exercise native stack-map roots and streamed Response state when
# Perry's runtime and stdlib are process-wide providers and generated code
# lives only in a later-loaded app image. The fixtures deliberately mirror the
# reporter's host boundary: each app is a two-module, app-only dylib; the GC
# fixture requests full pressure only after a completed invocation; and the
# Response fixture roots its exported Promise while sync, async, subclassed,
# and rejected streams are drained under normal and forced/verified GC.

repo_root=$(cd "$(dirname "$0")/.." && pwd)
fixture="$repo_root/tests/fixtures/issue_8075_provider_gc"
profile=${PERRY_PROVIDER_GC_PROFILE:-perry-dev}
target_dir=${CARGO_TARGET_DIR:-$repo_root/target}
perry=${PERRY_BIN:-$target_dir/$profile/perry}
real_cc=$(command -v cc)
host_os=$(uname -s)
host_arch=$(uname -m)

case "$host_os/$host_arch" in
  Darwin/arm64|Darwin/x86_64)
    library_extension=dylib
    runtime_filename=libperry_runtime.dylib
    stdlib_filename=libperry_stdlib.dylib
    if [[ "$host_arch" == arm64 ]]; then
      cargo_linker_env=CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER
    else
      cargo_linker_env=CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER
    fi
    ;;
  Linux/aarch64|Linux/x86_64)
    library_extension=so
    runtime_filename=libperry_runtime.so
    stdlib_filename=libperry_stdlib.so
    if [[ "$host_arch" == aarch64 ]]; then
      cargo_linker_env=CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER
    else
      cargo_linker_env=CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER
    fi
    ;;
  *)
    echo "SKIP: #8075 provider GC gate does not support $host_os/$host_arch"
    exit 0
    ;;
esac

if [[ ! -x "$perry" ]]; then
  echo "Perry compiler is missing: $perry" >&2
  echo "Build it with: cargo build --profile $profile -p perry" >&2
  exit 1
fi

scratch=$(mktemp -d "${TMPDIR:-/tmp}/perry-8075-provider.XXXXXX")
provider_source="$scratch/perry-provider-source"
provider_target_dir="$scratch/provider-target"
worktree_added=false
cleanup() {
  if [[ "$worktree_added" == true ]]; then
    git -C "$repo_root" worktree remove --force "$provider_source" >/dev/null 2>&1 || true
  fi
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM

# The runtime crate normally emits only an rlib. Build the provider from a
# disposable worktree so changing its crate type cannot race with or dirty the
# checkout running the gate. HEAD is also the compiler/provider identity this
# integration contract requires.
git -C "$repo_root" worktree add --detach "$provider_source" HEAD >/dev/null
worktree_added=true
runtime_manifest="$provider_source/crates/perry-runtime/Cargo.toml"
runtime_manifest_backup="$scratch/perry-runtime.Cargo.toml"
cp "$runtime_manifest" "$runtime_manifest_backup"
perl -0pi -e 's/crate-type = \["rlib"\]/crate-type = ["dylib"]/ or die "runtime crate-type marker missing\n"' "$runtime_manifest"

runtime_features="full,regex-engine,temporal,url-engine,string-normalize,intl-segmenter,intl-namespace,global-math,global-json,global-reflect,global-atomics,global-url,global-text,global-websocket,global-webcrypto,global-webfetch,proc-ipc,intl-locale,intl-datetime,diagnostics,mod-dgram,mod-http2-constants,mod-node-test,dyn-eval,keepalive-anchors,stdlib"
if [[ "$host_os" == Darwin ]]; then
  CARGO_TARGET_DIR="$provider_target_dir" cargo rustc \
    --manifest-path "$provider_source/Cargo.toml" \
    --profile "$profile" -p perry-runtime \
    --no-default-features --features "$runtime_features" -- \
    -C 'link-arg=-Wl,-install_name,@rpath/libperry_runtime.dylib' \
    -C link-arg=-framework -C link-arg=CoreFoundation \
    -C link-arg=-framework -C link-arg=Foundation
else
  CARGO_TARGET_DIR="$provider_target_dir" cargo rustc \
    --manifest-path "$provider_source/Cargo.toml" \
    --profile "$profile" -p perry-runtime \
    --no-default-features --features "$runtime_features" -- \
    -C 'link-arg=-Wl,-soname,libperry_runtime.so'
fi

provider_dir="$scratch/providers"
mkdir -p "$provider_dir"
runtime_library="$provider_dir/$runtime_filename"
cp "$provider_target_dir/$profile/libperry_runtime.$library_extension" "$runtime_library"
cp "$runtime_manifest_backup" "$runtime_manifest"

stdlib_manifest="$provider_source/tests/fixtures/issue_8075_provider_gc/stdlib-provider/Cargo.toml"
stdlib_linker="$provider_source/tests/fixtures/issue_8075_provider_gc/stdlib-linker.sh"
env \
  CARGO_TARGET_DIR="$provider_target_dir" \
  PERRY_ISSUE_8075_RUNTIME_LIBRARY="$runtime_library" \
  PERRY_ISSUE_8075_REAL_CC="$real_cc" \
  "$cargo_linker_env=$stdlib_linker" \
  cargo build --manifest-path "$stdlib_manifest" --profile provider

stdlib_library="$provider_dir/$stdlib_filename"
cp "$provider_target_dir/provider/libissue_8075_stdlib.$library_extension" "$stdlib_library"
if [[ "$host_os" == Darwin ]]; then
  install_name_tool -id '@rpath/libperry_runtime.dylib' "$runtime_library"
  install_name_tool -id '@rpath/libperry_stdlib.dylib' "$stdlib_library"
else
  nm -D "$stdlib_library" | grep -q ' T js_gc_init' || {
    echo "stdlib provider hid js_gc_init — not preemptible" >&2
    exit 1
  }
  readelf -d "$stdlib_library" | grep -Fq "Shared library: [$runtime_filename]" || {
    echo "stdlib provider is not bound to the separate runtime provider" >&2
    exit 1
  }
fi

app_link_dir="$scratch/app-linker"
mkdir -p "$app_link_dir"
ln -s "$provider_source/tests/fixtures/issue_8075_provider_gc/app-linker.sh" "$app_link_dir/cc"
app="$provider_dir/app.$library_extension"
env \
  PATH="$app_link_dir:$PATH" \
  PERRY_ISSUE_8075_REAL_CC="$real_cc" \
  PERRY_ISSUE_8075_RUNTIME_LIBRARY="$runtime_library" \
  PERRY_ISSUE_8075_STDLIB_LIBRARY="$stdlib_library" \
  PERRY_RS4GC=1 \
  PERRY_RUNTIME_DIR="$target_dir/$profile" \
  "$perry" compile \
    --no-codegen --no-auto-optimize --march generic \
    --output-type dylib -o "$app" "$fixture/perch_entry.ts"

if [[ "$host_os" == Darwin ]]; then
  dependencies=$(otool -L "$app")
  grep -Fq '@rpath/libperry_runtime.dylib' <<<"$dependencies" || {
    echo "app is not bound to the separate runtime provider" >&2
    exit 1
  }
  grep -Fq '@rpath/libperry_stdlib.dylib' <<<"$dependencies" || {
    echo "app is not bound to the separate stdlib provider" >&2
    exit 1
  }
  load_commands=$(otool -l "$app")
  grep -Fq 'sectname __perry_gcmap' <<<"$load_commands" || {
    echo "app GC map section was stripped by the macOS linker" >&2
    exit 1
  }
  symbols=$(nm -gU "$app" | awk 'NF >= 3 { symbol=$3; sub(/^_/, "", symbol); print symbol }')
else
  dependencies=$(readelf -d "$app")
  grep -Fq "Shared library: [$runtime_filename]" <<<"$dependencies"
  grep -Fq "Shared library: [$stdlib_filename]" <<<"$dependencies"
  symbols=$(nm -D --defined-only "$app" | awk 'NF >= 3 { print $3 }')
  readelf -SW "$app" | grep -Fq '.perry_gcmap'
fi

temporary_symbol=$(awk '/^__perry_wrap_perry_fn_.*__perchHttpEntry$/ { print; count++ } END { if (count != 1) exit 1 }' <<<"$symbols")
retained_symbol=$(awk '/^__perry_wrap_perry_fn_.*__perchRetainedEntry$/ { print; count++ } END { if (count != 1) exit 1 }' <<<"$symbols")

host="$scratch/issue-8075-host"
rustc --edition 2021 -O "$fixture/host.rs" -o "$host"
if [[ "$host_os" == Darwin ]]; then
  DYLD_LIBRARY_PATH="$provider_dir" "$host" \
    "$runtime_library" "$stdlib_library" "$app" \
    "$temporary_symbol" "$retained_symbol"
else
  LD_LIBRARY_PATH="$provider_dir" "$host" \
    "$runtime_library" "$stdlib_library" "$app" \
    "$temporary_symbol" "$retained_symbol"
fi

# #8038's executable parity fixture is also its app-only provider fixture. The
# environment guard suppresses its ordinary top-level invocation; the native
# host calls the exported async wrapper, roots the returned Promise in the
# provider runtime, and pumps it until fulfillment. Comparing stdout to the
# Node oracle proves that both chunks and EOF were observed, response/Headers/
# body identities remained stable, cookies survived, and stream errors rejected
# instead of hanging. Run once normally and once with moving-GC verification.
response_app="$provider_dir/issue-8038-response.$library_extension"
env \
  PATH="$app_link_dir:$PATH" \
  PERRY_ISSUE_8075_REAL_CC="$real_cc" \
  PERRY_ISSUE_8075_RUNTIME_LIBRARY="$runtime_library" \
  PERRY_ISSUE_8075_STDLIB_LIBRARY="$stdlib_library" \
  PERRY_RS4GC=1 \
  PERRY_RUNTIME_DIR="$target_dir/$profile" \
  "$perry" compile \
    --no-codegen --no-auto-optimize --march generic \
    --output-type dylib -o "$response_app" \
    "$repo_root/test-files/test_issue_8038_cross_module_response_stream.ts"

if [[ "$host_os" == Darwin ]]; then
  response_symbols=$(nm -gU "$response_app" | awk 'NF >= 3 { symbol=$3; sub(/^_/, "", symbol); print symbol }')
else
  response_symbols=$(nm -D --defined-only "$response_app" | awk 'NF >= 3 { print $3 }')
fi
response_entry=$(awk '/^__perry_wrap_perry_fn_.*__runIssue8038$/ { print; count++ } END { if (count != 1) exit 1 }' <<<"$response_symbols")

response_host="$scratch/issue-8038-response-host"
rustc --edition 2021 -O \
  "$repo_root/tests/fixtures/issue_8038_response_dylib/host.rs" \
  -o "$response_host"

run_response_fixture() {
  local mode=$1
  local output="$scratch/issue-8038-$mode.out"
  local stderr="$scratch/issue-8038-$mode.err"
  shift
  if [[ "$host_os" == Darwin ]]; then
    if ! env DYLD_LIBRARY_PATH="$provider_dir" \
      PERRY_ISSUE_8038_LIBRARY_HOST=1 "$@" \
      "$response_host" "$runtime_library" "$stdlib_library" \
      "$response_app" "$response_entry" >"$output" 2>"$stderr"; then
      cat "$stderr" >&2
      return 1
    fi
  else
    if ! env LD_LIBRARY_PATH="$provider_dir" \
      PERRY_ISSUE_8038_LIBRARY_HOST=1 "$@" \
      "$response_host" "$runtime_library" "$stdlib_library" \
      "$response_app" "$response_entry" >"$output" 2>"$stderr"; then
      cat "$stderr" >&2
      return 1
    fi
  fi
  diff -u \
    "$repo_root/test-parity/expected/test_issue_8038_cross_module_response_stream.txt" \
    "$output"
  if [[ "$mode" == forced ]]; then
    python3 "$repo_root/scripts/gc_evacuation_liveness_assert.py" \
      "$stderr" --probe "#8038 provider-dylib Response"
  fi
}

run_response_fixture normal
run_response_fixture forced \
  PERRY_GC_DIAG=1 \
  PERRY_GC_FORCE_EVACUATE=1 \
  PERRY_GC_VERIFY_EVACUATION=1 \
  PERRY_GC_SCHEDULE_SEED=8038 \
  PERRY_GC_SCHEDULE_RATE=1
