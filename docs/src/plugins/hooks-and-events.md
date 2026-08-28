# Hooks & Events

> **Status: wired** ([#189](https://github.com/PerryTS/perry/issues/189) closed). `api.registerHook`, `api.on`, `emitHook`, `emitEvent`, `invokeTool` all dispatch to `crates/perry-runtime/src/plugin.rs`. Snippets below are compile-link verified against [`docs/examples/plugins/{plugin,host}_snippets.ts`](https://github.com/PerryTS/perry/tree/main/docs/examples/plugins).

Perry plugins communicate through hooks, events, and tools.

## Hook Modes

Hooks support three execution modes:

### Filter Mode (default)

Each plugin receives data and returns (possibly modified) data. The output of one plugin becomes the input of the next:

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:hook-filter}}
```

### Action Mode

Plugins receive data but return value is ignored. Used for side effects. Pass
`mode = 1` to `registerHookEx`:

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:hook-action}}
```

### Waterfall Mode

Like filter mode, but specifically for accumulating/building up a result
through the chain. Pass `mode = 2` to `registerHookEx`:

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:hook-waterfall}}
```

## Hook Priority

Lower priority numbers run first. Use `registerHookEx` for explicit priority
and mode:

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:hook-priority}}
```

Default priority is 10 (the value `registerHook` passes implicitly).

## Event Bus

Plugins can communicate with each other through events:

### Emitting Events

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:emit-from-plugin}}
```

```typescript
{{#include ../../examples/plugins/host_snippets.ts:emit-event}}
```

### Listening for Events

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:on-event}}
```

## Tools

Plugins register callable tools (note the 3-arg shape: `name`, `description`,
`handler`):

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:register-tool}}
```

```typescript
{{#include ../../examples/plugins/host_snippets.ts:invoke-tool}}
```

## Configuration

Hosts can pass configuration to plugins via `setPluginConfig`:

```typescript
{{#include ../../examples/plugins/host_snippets.ts:init}}
```

```typescript
{{#include ../../examples/plugins/plugin_snippets.ts:read-config}}
```

## Introspection

Query loaded plugins and their registrations:

```typescript
{{#include ../../examples/plugins/host_snippets.ts:introspect}}
```

## Next Steps

- [Creating Plugins](creating-plugins.md) — Build a plugin
- [Overview](overview.md) — Plugin system overview
