# Creating Plugins

> **Status: wired** ([#189](https://github.com/PerryTS/perry/issues/189) closed). See [Plugin System Overview — Status](overview.md) for the full surface. Snippets below are compile-link verified by the doc-tests harness against [`docs/examples/plugins/plugin_snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/plugins/plugin_snippets.ts) and [`docs/examples/plugins/host_snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/plugins/host_snippets.ts).

Build Perry plugins as shared libraries that extend host applications.

## Step 1: Write the Plugin

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:counter-plugin}}
```

## Step 2: Compile as Shared Library

```bash
perry counter-plugin.ts --output-type dylib -o counter-plugin.dylib
```

The `--output-type dylib` flag tells Perry to produce a `.dylib` (macOS) or `.so` (Linux) instead of an executable.

Perry automatically:
- Generates `perry_plugin_abi_version()` returning the current ABI version
- Generates `plugin_activate(api_handle)` calling your `activate()` function
- Generates `plugin_deactivate()` calling your `deactivate()` function
- Exports symbols with `-rdynamic` for the host to find

## Step 3: Load from Host

```typescript,no-test
{{#include ../../examples/plugins/host_snippets.ts:imports}}

{{#include ../../examples/plugins/host_snippets.ts:load}}

{{#include ../../examples/plugins/host_snippets.ts:discover}}

{{#include ../../examples/plugins/host_snippets.ts:emit-hook}}

{{#include ../../examples/plugins/host_snippets.ts:invoke-tool}}
```

## Plugin API Reference

The `api: PluginApi` passed to `activate()` provides:

### Metadata

```typescript,no-test
api.setMetadata(name: string, version: string, description: string): void
```

### Hooks

```typescript,no-test
api.registerHook(name: string, handler: (ctx: unknown) => unknown): void
api.registerHookEx(name: string, handler: (ctx: unknown) => unknown, priority: number, mode: number): void
```

`registerHook` defaults to priority 10 / mode 0 (filter). Use `registerHookEx`
for explicit priority and mode (0=filter, 1=action, 2=waterfall). Lower
priority numbers run first.

### Tools

```typescript,no-test
api.registerTool(name: string, description: string, handler: (args: unknown) => unknown): void
```

Tools are invoked by name from the host.

### Configuration

```typescript,no-test
const value = api.getConfig(key: string)  // Read host-provided config
```

### Events

```typescript,no-test
api.on(event: string, handler: (data: unknown) => void): void  // Listen for events
api.emit(event: string, data: unknown): void                    // Emit to other plugins
```

## Next Steps

- [Hooks & Events](hooks-and-events.md) — Hook modes, event bus
- [Overview](overview.md) — Plugin system overview
