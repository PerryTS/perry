# Events

Perry widgets support native event handlers for user interaction. Every snippet
below is excerpted from
[`docs/examples/ui/events/snippets.ts`](../../examples/ui/events/snippets.ts) —
CI compiles and runs it on every PR, so the API drawn here is the API the
runtime exposes.

Event handlers are registered as **free functions** that take the widget handle
as the first argument. The widget handle itself is opaque (`number` at the
type level); perry's API is function-first throughout.

## onClick

```typescript
{{#include ../../examples/ui/events/snippets.ts:on-click}}
```

## onHover

Triggered when the cursor enters the widget.

```typescript
{{#include ../../examples/ui/events/snippets.ts:on-hover}}
```

> **Note**: Hover events are available on macOS, Windows, Linux, and Web. iOS
> and Android use touch interactions instead. The callback fires once on enter;
> if you need a "left" event you'll have to track it yourself.

## onDoubleClick

```typescript
{{#include ../../examples/ui/events/snippets.ts:on-double-click}}
```

## Keyboard Shortcuts

Register in-app keyboard shortcuts (active when the app is focused):

```typescript
{{#include ../../examples/ui/events/snippets.ts:keyboard}}
```

**Modifier bits:** `1` = Cmd (macOS) / Ctrl (Windows/Linux), `2` = Shift, `4` =
Option (macOS) / Alt (others), `8` = Control (macOS only). Combine by adding
— `3` = Cmd+Shift, `5` = Cmd+Option, etc.

Keyboard shortcuts are also available on [menu items](menus.md):

```typescript
{{#include ../../examples/ui/events/snippets.ts:menu-shortcut}}
```

### Global Hotkeys

Register a hotkey that fires system-wide, even when the app is in the
background:

```typescript
{{#include ../../examples/ui/events/snippets.ts:global-hotkey}}
```

**Platform support:** macOS uses Carbon `RegisterEventHotKey` (real
implementation). Linux, Windows, iOS, tvOS, visionOS, watchOS, and Android
log the registration and no-op — global hotkeys on those platforms require
OS-level portal / hook APIs that vary per OS.

## Clipboard

```typescript
{{#include ../../examples/ui/events/snippets.ts:clipboard}}
```

## Complete Example

```typescript
{{#include ../../examples/ui/events/complete.ts}}
```

## Next Steps

- [Menus](menus.md) — Menu bar and context menus with keyboard shortcuts
- [Widgets](widgets.md) — All available widgets
- [State Management](state.md) — Reactive state
