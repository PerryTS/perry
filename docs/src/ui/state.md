# State Management

Perry uses reactive state to automatically update the UI when data changes.
Every snippet below is excerpted from
[`docs/examples/ui/state/snippets.ts`](../../examples/ui/state/snippets.ts) —
CI compiles and runs it on every PR.

## Creating State

```typescript
{{#include ../../examples/ui/state/snippets.ts:creating}}
```

`State(initialValue)` creates a reactive state container.

## Reading and Writing

```typescript
{{#include ../../examples/ui/state/snippets.ts:read-write}}
```

Every `.set()` call re-renders the widget tree with the new value.

## Reactive Text

Template literals with `state.value` update automatically:

```typescript
{{#include ../../examples/ui/state/snippets.ts:reactive-text}}
```

This works because Perry detects `state.value` reads inside template literals
and creates reactive bindings.

## Binding Inputs to State

Input widgets expose an `onChange` callback. Forward that into a state's
`.set(...)` to keep the state in sync as the user types/toggles/drags:

```typescript
{{#include ../../examples/ui/state/snippets.ts:bind-textfield}}
```

Input control signatures:
- `TextField(placeholder, onChange)` — text input, `onChange: (value: string) => void`
- `SecureField(placeholder, onChange)` — password input, `onChange: (value: string) => void`
- `Toggle(label, onChange)` — boolean toggle, `onChange: (value: boolean) => void`
- `Slider(min, max, onChange)` — numeric slider, `onChange: (value: number) => void`
- `Picker(onChange)` — dropdown, `onChange: (index: number) => void`; items via `pickerAddItem`
- `WheelPicker(onChange)` — spinning selector on touch platforms and a scrollable selector elsewhere; items via `wheelPickerAddItem`

For programmatic-to-UI sync (state-drives-widget) use the dedicated binders:
`stateBindTextfield`, `stateBindSlider`, `stateBindToggle`, `stateBindTextNumeric`,
`stateBindVisibility`.

## onChange Callbacks

Listen for state changes with the free-function `stateOnChange`:

```typescript
{{#include ../../examples/ui/state/snippets.ts:on-change}}
```

## ForEach

Render a list from numeric state (the index count):

```typescript
{{#include ../../examples/ui/state/snippets.ts:foreach}}
```

> **Note:** `ForEach` iterates by index over a numeric state. Keep a count
> state in sync with your array, then read the items via `array.value[i]`
> inside the closure.

`ForEach` re-renders the list when the count state changes:

```typescript
{{#include ../../examples/ui/state/snippets.ts:foreach-mutate}}
```

## Conditional Rendering

Use state to conditionally show widgets:

```typescript
{{#include ../../examples/ui/state/snippets.ts:conditional}}
```

## Multi-State Text

Text can depend on multiple state values:

```typescript
{{#include ../../examples/ui/state/snippets.ts:multi-state}}
```

## State with Objects and Arrays

```typescript
{{#include ../../examples/ui/state/snippets.ts:object-state}}
```

> **Note**: State uses identity comparison. You must create a new array/object
> reference for changes to be detected. Mutating in-place without calling
> `.set()` with a new reference won't trigger updates.

## Complete Example

```typescript
{{#include ../../examples/ui/state/todo_app.ts}}
```

This program is built and run by CI (`scripts/run_doc_tests.sh`), so the
snippet above always matches the compiled artifact under
[`docs/examples/ui/state/todo_app.ts`](../../examples/ui/state/todo_app.ts).

## Next Steps

- [Events](events.md) — Click, hover, keyboard events
- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
