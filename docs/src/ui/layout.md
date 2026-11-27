# Layout

Perry provides layout containers that arrange child widgets using the
platform's native layout system. Every snippet below is excerpted from
[`docs/examples/ui/layout/snippets.ts`](../../examples/ui/layout/snippets.ts) —
CI compiles and runs it on every PR.

Layout helpers are free functions: `widgetAddChild(parent, child)`,
`stackSetAlignment(stack, value)`, `setPadding(w, top, left, bottom, right)`,
etc. Stack constructors take a numeric spacing followed by a child
array; everything else (alignment, distribution, padding, sizing) is applied
post-construction via the free functions on the widget handle.

## VStack

Arranges children vertically (top to bottom).

```typescript
{{#include ../../examples/ui/layout/snippets.ts:vstack}}
```

`VStack(spacing, children)` — the first argument is the gap in points between
children.

## HStack

Arranges children horizontally (left to right).

```typescript
{{#include ../../examples/ui/layout/snippets.ts:hstack}}
```

## ZStack

Layers children on top of each other (back to front). `ZStack()` takes no
constructor children — populate it with `widgetAddChild`:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:zstack}}
```

## ScrollView

A scrollable container. Built empty, then filled via `scrollviewSetChild`:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:scrollview}}
```

## LazyVStack

A vertically scrolling list that lazily renders items. More efficient than
`ScrollView` + `VStack` for thousands of rows — on macOS this is backed by
`NSTableView` so only rows in the visible rect are realized.

```typescript
{{#include ../../examples/ui/layout/snippets.ts:lazyvstack}}
```

When the underlying data changes, call `lazyvstackUpdate(handle, newCount)` to
refresh. Override the default 44pt row height with `lazyvstackSetRowHeight`.

## NavStack

A navigation container that supports push/pop navigation. Push a new view
with `navstackPush(stack, view, title)`; pop with `navstackPop(stack)`:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:navstack}}
```

## Links and routes

On the web target a page has a URL: a reader can bookmark it, share it, and use
the back button. `Link(text, href)` is an `<a href>`; a plain click on a
same-origin `href` changes the URL without reloading the page. Every other click
is the browser's: another origin, `mailto:` or `tel:`, a fragment of the page, a
modifier key or middle click, or a page opened from `file://`. The router keeps
the current route, `location.pathname + search + hash`:

- `routerNavigate(path)` pushes `path` onto the session history. It throws where
  the history refuses the URL: another origin or, from `file://`, another file.
- `routerOnChange(cb)` calls `cb(path)` after `routerNavigate`, a `Link` click,
  or back/forward. A listener stays for the page's life: register one per app. One
  registered during a notification waits for the next.
- `routerCurrentPath()` returns the current route, so a deep link opens on the
  right view.

```typescript,no-test
import { Link, routerCurrentPath, routerOnChange, Text, textSetString, VStack } from "perry/ui";

const view = Text(routerCurrentPath());
routerOnChange((path) => textSetString(view, path));
VStack(8, [Link("Requests", "/requests"), Link("Helpers", "/helpers"), view]);
```

Native backends have no URL: `routerNavigate` and `routerOnChange` do nothing,
and `routerCurrentPath()` returns `""`, except on HarmonyOS, where its value must
not be used. A `Link` there is a Text whose click opens an `http`, `https`,
`mailto` or `tel` URL in its default app; any other `href` does nothing. Like any
clickable Text it takes no keyboard focus. On tvOS, watchOS and HarmonyOS the text
cannot be activated.

A `javascript:` `href` is ignored on every target: the `Link` is only its text.

## Spacer

A flexible space that expands to fill available room.

```typescript
{{#include ../../examples/ui/layout/snippets.ts:spacer}}
```

Use `Spacer()` inside `HStack` or `VStack` to push widgets apart.

## Divider

A visual separator line.

```typescript
{{#include ../../examples/ui/layout/snippets.ts:divider}}
```

## Nesting Layouts

Layouts can be nested freely. This complete example is verified by CI:

```typescript
{{#include ../../examples/ui/layout/nesting.ts}}
```

## Child Management

Containers support dynamic child management via free functions:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:child-management}}
```

| Function | Description |
|----------|-------------|
| `widgetAddChild(parent, child)` | Append a child widget |
| `widgetAddChildAt(parent, child, index)` | Insert a child at a specific position |
| `widgetRemoveChild(parent, child)` | Remove a specific child |
| `widgetReorderChild(widget, fromIndex, toIndex)` | Move a child to a new position |
| `widgetClearChildren(widget)` | Remove all children |

## Stack Alignment

Control how children are aligned within a stack using `stackSetAlignment`:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:alignment}}
```

**VStack alignment** (cross-axis = horizontal):

| Value | Name | Effect |
|-------|------|--------|
| 5 | Leading | Children align to the leading (left) edge |
| 9 | CenterX | Children centered horizontally |
| 7 | Width | Children stretch to fill the stack's width |

**HStack alignment** (cross-axis = vertical):

| Value | Name | Effect |
|-------|------|--------|
| 3 | Top | Children align to the top |
| 12 | CenterY | Children centered vertically |
| 4 | Bottom | Children align to the bottom |

## Stack Distribution

Control how children share space within a stack using `stackSetDistribution`:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:distribution}}
```

| Value | Name | Behavior |
|-------|------|----------|
| 0 | Fill | Default. First resizable child fills remaining space |
| 1 | FillEqually | All children get equal size |
| 2 | FillProportionally | Children sized proportionally to their intrinsic content |
| 3 | EqualSpacing | Equal gaps between children |
| 4 | EqualCentering | Equal distance between child centers |

## Fill Parent

Pin a child's edges to its parent container:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:fill-parent}}
```

- `widgetMatchParentWidth(widget)` — stretch to fill parent's width
- `widgetMatchParentHeight(widget)` — stretch to fill parent's height
- `widgetSetMaxWidth(widget, maxWidth)` — fill the available width up to a cap and center horizontally (macOS and Web)

For a responsive content column, apply the cap to the column and put padding
inside it. This keeps a 32-point content inset on narrow windows and grows the
outside gutters on wide windows:

```typescript
{{#include ../../examples/ui/layout/max-width.ts}}
```

The cap includes padding and can be set before or after adding the widget to a
parent. Calling it again replaces the cap; it also replaces a previous fixed
`widgetSetWidth`. Negative and non-finite values are ignored; zero is valid.
macOS uses Auto Layout, and Web (JavaScript and WebAssembly) uses CSS `max-width`
with automatic side margins. iOS, tvOS, visionOS, watchOS, Android, GTK4,
Windows (including WinUI), and HarmonyOS currently accept this API as a no-op.

## Content Hugging

Control whether a widget resists being stretched beyond its intrinsic size:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:hugging}}
```

- **High priority** (250–750+): widget resists stretching, stays at its natural size
- **Low priority** (1–249): widget stretches to fill available space

## Overlay Positioning

For absolute positioning, add overlay children to any container:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:overlay}}
```

Overlay children are positioned absolutely relative to their parent — similar
to CSS `position: absolute`.

## Split Views

Create resizable split panes for sidebar layouts:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:split-view}}
```

The user can drag the divider to resize panes. On macOS this maps to
`NSSplitView`.

## Stacks with Built-in Padding

Create a stack with padding in a single call. The order is **top, left,
bottom, right**, matching the per-side `setPadding` overload:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:insets-stack}}
```

`HStackWithInsets(spacing, top, left, bottom, right)` is the horizontal
counterpart. Equivalent to creating a stack and then calling
`setPadding`, but more concise. Children are added via
`widgetAddChild` rather than the constructor array.

## Detaching Hidden Views

By default, hidden children still occupy space in a stack. To collapse them:

```typescript
{{#include ../../examples/ui/layout/snippets.ts:detaches-hidden}}
```

## Common Layout Patterns

### Centered content

```typescript
{{#include ../../examples/ui/layout/snippets.ts:pattern-centered}}
```

### Search row that fills the width

```typescript
{{#include ../../examples/ui/layout/snippets.ts:pattern-search-row}}
```

### Floating badge / overlay

```typescript
{{#include ../../examples/ui/layout/snippets.ts:pattern-floating-badge}}
```

### Toolbar with spacers

```typescript
{{#include ../../examples/ui/layout/snippets.ts:pattern-toolbar}}
```

## Next Steps

- [Styling](styling.md) — Colors, padding, sizing
- [Widgets](widgets.md) — All available widgets
- [State Management](state.md) — Dynamic UI with state
