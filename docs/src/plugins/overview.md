# Plugin System Overview

> **Status: wired** ([#189](https://github.com/PerryTS/perry/issues/189) closed). Receiver-less calls (`loadPlugin`, `listPlugins`, `emitHook`, `invokeTool`, ...) and `PluginApi` instance methods (`api.registerHook`, `api.registerTool`, ...) dispatch through `crates/perry-codegen/src/lower_call.rs::PERRY_PLUGIN_TABLE` and `PERRY_PLUGIN_INSTANCE_TABLE`. TypeScript surface lives in `types/perry/plugin/index.d.ts`. Host-side snippets below are compile-link verified by the doc-tests harness against [`docs/examples/plugins/host_snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/plugins/host_snippets.ts); plugin-side `activate(api)` snippets against [`docs/examples/plugins/plugin_snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/plugins/plugin_snippets.ts).

Perry supports native plugins as shared libraries (`.dylib`/`.so`). Plugins extend Perry applications with custom hooks, tools, services, and routes.

## How It Works

1. A plugin is a Perry-compiled shared library with `activate(api)` and `deactivate()` entry points
2. The host application loads plugins with `loadPlugin(path)`
3. Plugins register hooks, tools, and services via the API handle
4. The host dispatches events to plugins via `emitHook(name, data)`

```
Host Application
    ↓ loadPlugin("./my-plugin.dylib")
    ↓ calls plugin_activate(api_handle)
Plugin
    ↓ api.registerHook("beforeSave", callback)
    ↓ api.registerTool("format", callback)
Host
    ↓ emitHook("beforeSave", data) → plugin callback runs
```

## Quick Example

### Plugin (compiled with `--output-type dylib`)

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:counter-plugin}}
```

```bash
perry my-plugin.ts --output-type dylib -o my-plugin.dylib
```

### Host Application

```typescript,no-test
{{#include ../../examples/plugins/host_snippets.ts:imports}}

{{#include ../../examples/plugins/host_snippets.ts:load}}

{{#include ../../examples/plugins/host_snippets.ts:introspect}}

{{#include ../../examples/plugins/host_snippets.ts:emit-hook}}

{{#include ../../examples/plugins/host_snippets.ts:invoke-tool}}
```

## Plugin ABI

Plugins must export these symbols:
- `perry_plugin_abi_version()` — Returns ABI version (for compatibility checking)
- `plugin_activate(api_handle)` — Called when plugin is loaded
- `plugin_deactivate()` — Called when plugin is unloaded

Perry generates these automatically from your `activate`/`deactivate` exports.

## Native Extensions

Perry also supports **native extensions** — packages that bundle platform-specific Rust/Swift/JNI code and compile directly into your binary. These are used for accessing platform APIs like the App Store review prompt or StoreKit in-app purchases.

See [Native Extensions](native-extensions.md) for details.

## Next Steps

- [Creating Plugins](creating-plugins.md) — Build a plugin step by step
- [Hooks & Events](hooks-and-events.md) — Hook modes, event bus, tools
- [Native Extensions](native-extensions.md) — Extensions with platform-native code
- [App Store Review](appstore-review.md) — Native review prompt (iOS/Android)
