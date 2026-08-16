#!/usr/bin/env bash
set -euo pipefail

# #8037: run the exact #8034 production route through Next's generated
# AppRouteRouteModule.handle while Perry is embedded as an app-only dylib.
# Runtime and stdlib/HTTP are separate, eagerly relocated provider images.

repo_root=$(cd "$(dirname "$0")/.." && pwd)
fixture="$repo_root/tests/fixtures/next-app-route"
profile=${PERRY_NEXT_PROFILE:-perry-dev}
target_dir=${CARGO_TARGET_DIR:-$repo_root/target}
perry=${PERRY_BIN:-$target_dir/$profile/perry}
port=${PERRY_NEXT_PORT:-3100}
cold_starts=${PERRY_NEXT_COLD_STARTS:-10}
verifications_per_start=${PERRY_NEXT_VERIFICATIONS_PER_START:-10}
cargo_jobs=${PERRY_NEXT_CARGO_JOBS:-2}
real_cc=$(command -v cc)
host_os=$(uname -s)
host_arch=$(uname -m)
forbidden_diagnostics='generated handler bypassed|\[perry-gc\].*SKIPPED|unsettled.await|unimplemented|compatibility.fallback'

find_llvm22_clang() {
    local candidate
    local -a candidates=()
    if [[ -n ${PERRY_LLVM_CLANG:-} ]]; then
        candidates+=("$PERRY_LLVM_CLANG")
    fi
    if [[ -n ${LLVM_SYS_221_PREFIX:-} ]]; then
        candidates+=("$LLVM_SYS_221_PREFIX/bin/clang")
    fi
    if [[ "$host_os" == Darwin ]]; then
        candidates+=(
            /opt/homebrew/opt/llvm@22/bin/clang
            /opt/homebrew/opt/llvm/bin/clang
            /usr/local/opt/llvm@22/bin/clang
            /usr/local/opt/llvm/bin/clang
        )
    fi
    candidates+=(clang-22 clang)

    for candidate in "${candidates[@]}"; do
        if [[ "$candidate" != */* ]]; then
            candidate=$(command -v "$candidate" 2>/dev/null || true)
        fi
        if [[ -x "$candidate" ]] && "$candidate" --version 2>/dev/null | head -n 1 | grep -Eq 'version 22\.'; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

find_llvm22_opt() {
    local candidate
    local -a candidates=()
    if [[ -n ${PERRY_LLVM_OPT:-} ]]; then
        candidates+=("$PERRY_LLVM_OPT")
    fi
    if [[ -n ${LLVM_SYS_221_PREFIX:-} ]]; then
        candidates+=("$LLVM_SYS_221_PREFIX/bin/opt")
    fi
    candidates+=("$(dirname "$llvm_clang")/opt" opt-22 opt)

    for candidate in "${candidates[@]}"; do
        if [[ "$candidate" != */* ]]; then
            candidate=$(command -v "$candidate" 2>/dev/null || true)
        fi
        if [[ -x "$candidate" ]] && "$candidate" --version 2>/dev/null | head -n 1 | grep -Eq 'LLVM version 22\.'; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

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
        echo "SKIP: Next App Route dylib gate does not support $host_os/$host_arch"
        exit 0
        ;;
esac

for value in "$cold_starts" "$verifications_per_start" "$port" "$cargo_jobs"; do
    if [[ ! "$value" =~ ^[1-9][0-9]*$ ]]; then
        echo "cold starts, verifications, port, and cargo jobs must be positive integers" >&2
        exit 1
    fi
done
if [[ ! -x "$perry" ]]; then
    echo "Perry compiler is missing: $perry" >&2
    echo "Build it with: cargo build --profile $profile -p perry" >&2
    exit 1
fi
if ! llvm_clang=$(find_llvm22_clang); then
    echo "LLVM 22 clang is required for Perry's LLVM 22 textual IR" >&2
    echo "Set PERRY_LLVM_CLANG to the matching clang binary" >&2
    exit 1
fi
if ! llvm_opt=$(find_llvm22_opt); then
    echo "LLVM 22 opt is required alongside Perry's LLVM 22 clang" >&2
    echo "Set PERRY_LLVM_OPT to the matching opt binary" >&2
    exit 1
fi
commit=$(git -C "$repo_root" rev-parse HEAD)
next_version=$(node -p "require('$fixture/package-lock.json').packages['node_modules/next'].version")
echo "Perry commit: $commit"
echo "Next version: $next_version"
echo "Compile mode: app-only dylib; unified runtime and stdlib/HTTP provider graph"
echo "LLVM compiler: $($llvm_clang --version | head -n 1)"
echo "LLVM optimizer: $($llvm_opt --version | head -n 1)"

scratch=$(mktemp -d "${TMPDIR:-/tmp}/perry-next-app-route.XXXXXX")
runtime_source="$scratch/runtime-source"
provider_target_dir=${PERRY_NEXT_PROVIDER_TARGET_DIR:-$scratch/provider-target}
providers="$scratch/providers"
host_pid=""
worktree_added=false
cleanup() {
    if [[ -n "$host_pid" ]] && kill -0 "$host_pid" 2>/dev/null; then
        kill "$host_pid" 2>/dev/null || true
        wait "$host_pid" 2>/dev/null || true
    fi
    if [[ "$worktree_added" == true ]]; then
        git -C "$repo_root" worktree remove --force "$runtime_source" >/dev/null 2>&1 || true
    fi
    rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
mkdir -p "$provider_target_dir" "$providers"

if [[ ${PERRY_NEXT_SKIP_NODE_ORACLE:-0} != 1 ]]; then
    PERRY_NEXT_ORACLE_PORT=$((port + 1)) "$repo_root/tests/test_next_app_route_node_oracle.sh"
elif [[ ! -f "$fixture/.next/server/app/api/benchmark/route.js" ]]; then
    echo "PERRY_NEXT_SKIP_NODE_ORACLE=1 requires an existing fixture build" >&2
    exit 1
fi

route_bundle="$fixture/.next/server/app/api/benchmark/route.js"
node - "$route_bundle" <<'NODE'
const generated = require(require("node:path").resolve(process.argv[2]));
if (typeof generated.routeModule?.handle !== "function") {
  throw new Error("production routeModule.handle is missing");
}
if (generated.routeModule?.definition?.pathname !== "/api/benchmark") {
  throw new Error("production routeModule has the wrong pathname");
}
NODE

app="$providers/next-app-route.$library_extension"
compile_log="$scratch/compile.log"
(
    cd "$fixture"
    env \
        PERRY_NO_AUTO_OPTIMIZE=1 \
        PERRY_DISABLE_WELL_KNOWN=1 \
        PERRY_LLVM_INPROCESS="${PERRY_LLVM_INPROCESS:-0}" \
        PERRY_LLVM_CLANG="$llvm_clang" \
        PERRY_LLVM_OPT="$llvm_opt" \
        PERRY_LL_SIZE_OPT="${PERRY_LL_SIZE_OPT:-0}" \
        PERRY_CODEGEN_UNIT_BYTES="${PERRY_CODEGEN_UNIT_BYTES:-8388608}" \
        PERRY_MODULE_JOBS="${PERRY_MODULE_JOBS:-1}" \
        PERRY_CODEGEN_UNIT_JOBS="${PERRY_CODEGEN_UNIT_JOBS:-1}" \
        PERRY_RUNTIME_DIR="$target_dir/$profile" \
        "$perry" compile --no-auto-optimize --output-type dylib \
        -o "$app" perry-host.js
) 2>&1 | tee "$compile_log"
[[ -f "$app" ]] || { echo "Perry did not emit the app dylib" >&2; exit 1; }
if grep -Eiq "$forbidden_diagnostics" "$compile_log"; then
    echo "forbidden Perry diagnostic during app compilation" >&2
    grep -Ein "$forbidden_diagnostics" "$compile_log" | sed -n '1,120p' >&2
    exit 1
fi

required_symbols="$scratch/app-required-symbols"
if [[ "$host_os" == Darwin ]]; then
    nm -u "$app" \
        | awk 'NF >= 1 && $NF ~ /^_(js_|perry_)/ { print $NF }' \
        | sort -u >"$required_symbols"
else
    nm -D --undefined-only "$app" \
        | awk 'NF >= 1 && $NF ~ /^(js_|perry_)/ { print $NF }' \
        | sort -u >"$required_symbols"
fi
if [[ ! -s "$required_symbols" ]]; then
    echo "app dylib has no undefined Perry ABI symbols" >&2
    exit 1
fi

git -C "$repo_root" worktree add --detach "$runtime_source" HEAD >/dev/null
worktree_added=true
runtime_manifest="$runtime_source/crates/perry-runtime/Cargo.toml"
perl -0pi -e 's/crate-type = \["rlib"\]/crate-type = ["rlib", "dylib"]/ or die "runtime crate-type marker missing\n"' "$runtime_manifest"

# Build the runtime dylib and the stdlib/HTTP provider from one resolved Cargo
# graph. Rust's internal symbols include a crate disambiguator, so independently
# resolved provider and runtime builds are not interchangeable even when their
# public C ABI is identical.
provider_source="$runtime_source/crates/next-app-route-provider"
cp -R "$fixture/provider" "$provider_source"
perl -0pi -e '
    s#\.\./\.\./\.\./\.\./crates/perry-stdlib#../perry-stdlib#g;
    s#\.\./\.\./\.\./\.\./crates/perry-ext-http#../perry-ext-http#g;
    s/\n\[profile\.provider\].*?\n\[workspace\]\n/\n/s
        or die "provider workspace/profile markers missing\n";
' "$provider_source/Cargo.toml"
perl -0pi -e 's/("crates\/perry-runtime",\n)/$1    "crates\/next-app-route-provider",\n/ or die "workspace member marker missing\n"' \
    "$runtime_source/Cargo.toml"

runtime_build_library="$provider_target_dir/$profile/libperry_runtime.$library_extension"
provider_linker="$fixture/provider-linker.sh"
env \
    CARGO_TARGET_DIR="$provider_target_dir" \
    PERRY_NEXT_RUNTIME_LIBRARY="$runtime_build_library" \
    PERRY_NEXT_REQUIRED_SYMBOLS="$required_symbols" \
    PERRY_NEXT_REAL_CC="$real_cc" \
    "$cargo_linker_env=$provider_linker" \
    cargo build --manifest-path "$runtime_source/Cargo.toml" \
        --profile "$profile" --jobs "$cargo_jobs" \
        -p perry-runtime -p next-app-route-provider

runtime_library="$providers/$runtime_filename"
cp "$runtime_build_library" "$runtime_library"
stdlib_library="$providers/$stdlib_filename"
cp "$provider_target_dir/$profile/libnext_app_route_provider.$library_extension" "$stdlib_library"
if [[ "$host_os" == Darwin ]]; then
    install_name_tool -id '@rpath/libperry_runtime.dylib' "$runtime_library"
    install_name_tool -id '@rpath/libperry_stdlib.dylib' "$stdlib_library"
    available_symbols="$scratch/available-symbols"
    {
        nm -gU "$runtime_library" | awk 'NF >= 3 { print $3 }'
        nm -gU "$stdlib_library" | awk 'NF >= 3 { print $3 }'
    } | sort -u >"$available_symbols"
else
    available_symbols="$scratch/available-symbols"
    {
        nm -D --defined-only "$runtime_library" | awk 'NF >= 3 { print $3 }'
        nm -D --defined-only "$stdlib_library" | awk 'NF >= 3 { print $3 }'
    } | sort -u >"$available_symbols"
fi
missing_symbols="$scratch/missing-symbols"
comm -23 "$required_symbols" "$available_symbols" >"$missing_symbols"
if [[ -s "$missing_symbols" ]]; then
    echo "provider images do not satisfy the app ABI:" >&2
    sed -n '1,120p' "$missing_symbols" >&2
    exit 1
fi

# The host is C, not Rust (#8205): a Rust executable would carry rustc's
# System-allocator shim, and the stdlib provider's `__rust_dealloc` import must
# reach the runtime image's mimalloc-backed shim instead. See provider-host.c.
host="$scratch/provider-host"
"$real_cc" -O2 -o "$host" "$fixture/provider-host.c" -ldl
provider_abi=$(shasum -a 256 "$available_symbols" | awk '{print $1}')
echo "Provider ABI hash: $provider_abi"

for cold_start in $(seq 1 "$cold_starts"); do
    host_log="$scratch/host-$cold_start.log"
    (
        # Run from the fixture root, the release-tier layout with production
        # evidence (tests/release/packages/next-app-route/fixture.sh): Next
        # resolves `.next/routes-manifest.json` against the working directory
        # on every request, so serving from `.next/server` 500s each request
        # with ENOENT. The old reason to sit in `.next/server` — the webpack
        # runtime resolving `./chunks/*.js` from the server root — is gone:
        # computed relative chunk requires resolve against the route bundle
        # since #8146.
        cd "$fixture"
        if [[ "$host_os" == Darwin ]]; then
            PORT="$port" HOSTNAME=127.0.0.1 DYLD_LIBRARY_PATH="$providers" \
                "$host" "$runtime_library" "$stdlib_library" "$app"
        else
            PORT="$port" HOSTNAME=127.0.0.1 LD_LIBRARY_PATH="$providers" \
                "$host" "$runtime_library" "$stdlib_library" "$app"
        fi
    ) >"$host_log" 2>&1 &
    host_pid=$!

    ready=false
    for _ in $(seq 1 240); do
        if curl --fail --silent --output /dev/null \
            "http://127.0.0.1:$port/api/benchmark?id=ready&iterations=1"; then
            ready=true
            break
        fi
        if ! kill -0 "$host_pid" 2>/dev/null; then
            echo "provider host exited during cold start $cold_start" >&2
            sed -n '1,240p' "$host_log" >&2
            exit 1
        fi
        sleep 0.25
    done
    if [[ "$ready" != true ]]; then
        echo "provider host was not ready during cold start $cold_start" >&2
        sed -n '1,240p' "$host_log" >&2
        exit 1
    fi

    for verification in $(seq 1 "$verifications_per_start"); do
        BASE_URL="http://127.0.0.1:$port" node "$fixture/verify.mjs"
        echo "PASS: cold start $cold_start/$cold_starts, verifier $verification/$verifications_per_start"
    done
    kill "$host_pid" 2>/dev/null || true
    wait "$host_pid" 2>/dev/null || true
    host_pid=""
    if grep -Eiq "$forbidden_diagnostics" "$host_log"; then
        echo "forbidden Perry diagnostic during cold start $cold_start" >&2
        sed -n '1,240p' "$host_log" >&2
        exit 1
    fi
done

total=$((cold_starts * verifications_per_start))
echo "PASS: $total production App Route verifier repetitions through app-only dylib providers"
