#!/bin/bash
# End-to-end test: build a host and a plugin, load the plugin, fire a hook,
# verify the hook callback ran. Cross-platform via extension detection:
#   - macOS:   host=host      plugin=plugin.dylib
#   - Linux:   host=host      plugin=plugin.so
#   - Windows: host=host.exe  plugin=plugin.dll
#
# This exercises the full perry/plugin pipeline:
#   1. compile-as-dylib link path (compile.rs)
#   2. host-side symbol export (link/mod.rs /DEF on Windows, -u/-rdynamic elsewhere)
#   3. runtime plugin loader (plugin.rs: open_library / lookup_symbol / activate)
#   4. hook dispatch (plugin.rs: perry_plugin_emit_hook)
#   5. unload teardown (plugin.rs: perry_plugin_unload)
#
# On any host where the build is unsupported (e.g. cross-compile without a
# matching toolchain), the test skips with an explanatory message rather than
# failing — the per-platform support matrix is exercised in CI per runner OS.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"

if [ ! -f "$PERRY" ]; then
    PERRY="$SCRIPT_DIR/../target/debug/perry"
fi
if [ ! -f "$PERRY" ]; then
    echo "SKIP: perry binary not found (build with cargo build --release)"
    exit 0
fi

# Platform detection
case "$(uname -s 2>/dev/null || echo Windows)" in
    Darwin)
        HOST_BIN="host"
        PLUGIN_EXT="dylib"
        ;;
    Linux)
        HOST_BIN="host"
        PLUGIN_EXT="so"
        ;;
    MINGW*|MSYS*|CYGWIN*|Windows)
        HOST_BIN="host.exe"
        PLUGIN_EXT="dll"
        ;;
    *)
        echo "SKIP: unsupported host $(uname -s 2>/dev/null || echo unknown)"
        exit 0
        ;;
esac

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# --- Plugin source ---------------------------------------------------------
# Registers a filter hook that uppercases a `name` field, and a tool that
# returns a static greeting. The host exercises both.

cat > "$TMPDIR/plugin.ts" << 'PLUGIN'
import type { PluginApi } from "perry/plugin"

export function activate(api: PluginApi) {
    api.setMetadata("test-plugin", "1.0.0", "End-to-end plugin test")

    api.registerHook("transform", (data: any) => {
        if (data && typeof data.name === "string") {
            data.name = data.name.toUpperCase()
        }
        return data
    })

    api.registerTool("greet", "test greeting", (args: any) => {
        return `hello, ${args?.who ?? "world"}`
    })
}

export function deactivate() {
    // no-op; teardown is exercised by the host's unloadPlugin call
}
PLUGIN

# --- Host source -----------------------------------------------------------
# Loads the plugin, fires the hook, invokes the tool, then unloads.

cat > "$TMPDIR/host.ts" << 'HOST'
import {
    initPlugins,
    loadPlugin,
    unloadPlugin,
    emitHook,
    invokeTool,
} from "perry/plugin"

function main(): number {
    initPlugins()

    const pluginPath = process.argv[2]
    if (!pluginPath) {
        console.error("usage: host <plugin-path>")
        return 2
    }

    const id = loadPlugin(pluginPath)
    if (id === 0) {
        console.error("FAIL: loadPlugin returned 0")
        return 1
    }

    const transformed = emitHook("transform", { name: "perry" })
    console.log(`name=${transformed.name}`)

    const greeting = invokeTool("greet", { who: "windows" })
    console.log(`greeting=${greeting}`)

    unloadPlugin(id)
    return 0
}

main()
HOST

# --- Compile ---------------------------------------------------------------

cd "$TMPDIR"

echo "Compiling plugin to $PLUGIN_EXT..."
"$PERRY" compile plugin.ts --output-type dylib -o "plugin.$PLUGIN_EXT" 2>&1

echo "Compiling host to $HOST_BIN..."
"$PERRY" compile host.ts -o "$HOST_BIN" 2>&1

if [ ! -f "plugin.$PLUGIN_EXT" ]; then
    echo "FAIL: plugin.$PLUGIN_EXT was not produced"
    exit 1
fi
if [ ! -f "$HOST_BIN" ]; then
    echo "FAIL: $HOST_BIN was not produced"
    exit 1
fi

# --- Run -------------------------------------------------------------------

echo "Running host with plugin..."
# `set -e` would short-circuit on a non-zero host exit before `EXIT=$?` ran;
# capture the exit code in the same subshell assignment so the diagnostic
# block below always sees the real status. Pass `./plugin.<ext>` (not a bare
# filename) so the OS loader resolves it from the test's cwd instead of
# any PATH / DLL-search-list entry — a bare `plugin.dll` on Windows would
# silently pick up an unrelated file from C:\Windows\System32 if one ever
# landed there.
set +e
OUTPUT=$(./"$HOST_BIN" "./plugin.$PLUGIN_EXT" 2>&1)
EXIT=$?
set -e

if [ "$EXIT" -ne 0 ]; then
    echo "FAIL: host exited with status $EXIT"
    echo "$OUTPUT"
    exit 1
fi

echo "Output:"
echo "$OUTPUT"
echo ""

# Hook should have uppercased the name (filter mode returns transformed ctx)
if ! echo "$OUTPUT" | grep -qF "name=PERRY"; then
    echo "FAIL: hook did not transform 'perry' to 'PERRY'"
    echo "Expected 'name=PERRY' in output, got:"
    echo "$OUTPUT"
    exit 1
fi

# Tool invocation should have produced a greeting
if ! echo "$OUTPUT" | grep -qF "greeting=hello, windows"; then
    echo "FAIL: tool did not return expected greeting"
    echo "Expected 'greeting=hello, windows' in output, got:"
    echo "$OUTPUT"
    exit 1
fi

echo "PASS: end-to-end plugin lifecycle (load -> hook -> tool -> unload)"
exit 0
