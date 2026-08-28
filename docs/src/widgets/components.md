# Widget Components & Modifiers

Available components and modifiers for widgets.

> **Status:** this page mixes (a) tiny fragments showing component shape —
> rendered as plain `text` because they're not standalone declarations and
> can't compile — and (b) one full verified Widget at the bottom that
> compile-links via
> [`docs/examples/widgets/snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/widgets/snippets.ts).
> The doc-tests harness can pass `--app-bundle-id` from a
> `// widget-bundle-id:` banner ([#194](https://github.com/PerryTS/perry/issues/194));
> end-to-end targets also require their platform SDK. Modifier syntax is
> deliberately limited to **inline option-object arguments** — e.g.
> `Text("hi", { font: "title", color: "red" })` and
> `VStack([...], { padding: 16 })`. Method-style chains such as
> `Text("hi").font("title")` are rejected with an actionable compile-time
> diagnostic instead of being silently ignored; that behavior closed
> [#195](https://github.com/PerryTS/perry/issues/195). The end-to-end reference is
> [`examples/widget_demo.ts`](https://github.com/PerryTS/perry/blob/main/examples/widget_demo.ts).

## Text

```text
Text("Hello, World!")
Text(`${entry.name}: ${entry.value}`)
```

### Text Modifiers

```text
const t = Text("Styled", {
  font: "title",          // title, headline, body, caption, etc.
  color: "blue",          // named color or hex
  fontWeight: "bold",
});
```

## Layout

### VStack

```text
VStack([
  Text("Top"),
  Text("Bottom"),
])
```

### HStack

```text
HStack([
  Text("Left"),
  Spacer(),
  Text("Right"),
])
```

### ZStack

```text
ZStack([
  Image("background"),
  Text("Overlay"),
])
```

## Spacer

Flexible space that expands to fill available room:

```text
HStack([
  Text("Left"),
  Spacer(),
  Text("Right"),
])
```

## Image

Display SF Symbols or asset images:

```text
Image("star.fill")           // SF Symbol
Image("cloud.sun.rain.fill") // SF Symbol
```

## ForEach

Iterate over array entry fields to render a list of components:

```text
ForEach(entry.items, (item) =>
  HStack([
    Text(item.name),
    Spacer(),
    Text(`${item.value}`),
  ])
)
```

## Divider

A visual separator line:

```text
VStack([
  Text("Above"),
  Divider(),
  Text("Below"),
])
```

## Label

A label with text and an SF Symbol icon:

```text
Label("Downloads", "arrow.down.circle")
Label(`${entry.count} items`, "folder.fill")
```

## Gauge

A circular or linear progress indicator:

```text
Gauge(entry.progress, 0, 100)       // value, min, max
Gauge(entry.battery, 0, 1.0)
```

## Modifiers

Widget components use inline option objects. Chained modifier calls are a
compile-time error so styling cannot disappear silently. The examples below
use the form that reaches codegen, as does the
[Complete Example](#complete-example).

### Font

```text
Text("Title", { font: "title" })
Text("Body", { font: "body" })
Text("Caption", { font: "caption" })
```

### Color

```text
Text("Red text", { color: "red" })
Text("Custom", { color: "#FF6600" })
```

### Padding

```text
VStack([...], { padding: 16 })
```

### Frame

```text
Text("Fixed", { frame: { width: 120, height: 40 } })
```

### Max Width

```text
VStack([...], { maxWidth: "infinity" }) // expand to fill available width
```

### Minimum Scale Factor

Allow text to shrink to fit:

```text
Text("Long text", { minimumScaleFactor: 0.5 })
```

### Container Background

Set background color for the widget container:

```text
VStack([...], { containerBackground: "blue" })
```

### Widget URL

Make the widget tappable with a deep link:

```text
VStack([...], { url: "myapp://detail/123" })
```

Edge-specific `paddingEdge(...)` chains are not part of the current widget
modifier surface. Use uniform `padding`, or compose nested stacks and spacers
when individual edges need different spacing.

## Conditionals

Render different components based on entry data:

```text
render: (entry) =>
  VStack([
    entry.isOnline
      ? Text("Online", { color: "green" })
      : Text("Offline", { color: "red" }),
  ]),
```

## Complete Example

The full Widget below is the verified extract — it compile-links on the host
LLVM target and uses the inline-options modifier form that round-trips through
the codegen.

```typescript
{{#include ../../examples/widgets/snippets.ts:stats-widget}}
```

## Next Steps

- [Creating Widgets](creating-widgets.md) — Widget() API
- [Overview](overview.md) — Widget system overview
