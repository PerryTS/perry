# Creating Widgets

Define home screen widgets using the `Widget()` function.

> **Status:** the full `Widget({...})` snippets on this page compile-link
> cleanly on the host LLVM target via
> [`docs/examples/widgets/snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/widgets/snippets.ts),
> so the API shapes are verified against the codegen. The actual cross-compile
> targets (`--target ios-widget`/`android-widget`/`watchos-widget`/`wearos-tile`)
> can be driven by the doc-tests harness with a `// widget-bundle-id:` banner
> ([#194](https://github.com/PerryTS/perry/issues/194)). These shared snippets
> do not request those targets, so their regular CI coverage remains the host
> compile/link path; SDK-backed target coverage runs only where the required
> Xcode or Android toolchain is installed.
> For the canonical end-to-end shape see
> [`examples/widget_demo.ts`](https://github.com/PerryTS/perry/blob/main/examples/widget_demo.ts).
> Fragments below that show partial syntax (just the `entryFields` object,
> just a `render:` body, etc.) are rendered as plain text — the full
> declarations they appear inside are covered by the verified anchors.

## Widget Declaration

```typescript
{{#include ../../examples/widgets/snippets.ts:weather-widget}}
```

## Widget Options

| Property | Type | Description |
|----------|------|-------------|
| `kind` | `string` | Unique identifier for the widget |
| `displayName` | `string` | Name shown in widget gallery |
| `description` | `string` | Description in widget gallery |
| `entryFields` | `object` | Data fields with types (`"string"`, `"number"`, `"boolean"`, arrays, optionals, objects) |
| `render` | `function` | Render function receiving entry data, returns widget tree. Optional 2nd param for family. |
| `config` | `object` | Configurable parameters the user can edit (see below) |
| `provider` | `function` | Timeline provider function for dynamic data (see below) |
| `appGroup` | `string` | App group identifier for sharing data with the host app |

## Entry Fields

Entry fields define the data your widget displays. Each field has a name and type:

```text
entryFields: {
  title: "string",
  count: "number",
  isActive: "boolean",
}
```

### Array, Optional, and Object Fields

Entry fields support richer types beyond primitives:

```text
entryFields: {
  items: [{ name: "string", value: "number" }],  // Array of objects
  subtitle: "string?",                             // Optional string
  stats: { wins: "number", losses: "number" },     // Nested object
}
```

These compile to a Swift `TimelineEntry` struct:

```swift
struct WeatherEntry: TimelineEntry {
    let date: Date
    let temperature: Double
    let condition: String
    let location: String
}
```

## Conditionals in Render

Use ternary expressions for conditional rendering:

```typescript
{{#include ../../examples/widgets/snippets.ts:conditional-render}}
```

## Template Literals

Template literals in widget text are compiled to Swift string interpolation:

```typescript
{{#include ../../examples/widgets/snippets.ts:template-literal}}
```

## Configuration Parameters

The `config` field defines user-editable parameters that appear in the widget's edit UI:

```typescript
{{#include ../../examples/widgets/snippets.ts:city-weather-config}}
```

## Provider Function

The `provider` field defines a timeline provider that fetches data for the widget:

```typescript
{{#include ../../examples/widgets/snippets.ts:stock-widget}}
```

> Note: chain-style modifiers (`.font("title").color("green")`) are rejected
> with a compile-time diagnostic so styling cannot disappear silently; this
> behavior closed [#195](https://github.com/PerryTS/perry/issues/195). The
> verified extract uses the supported inline-options form
> `Text("...", { font: "title" })`.

### Placeholder Data

When the widget has no data yet (e.g., first load), the provider can return placeholder data by providing a `placeholder` field:

```text
Widget({
  kind: "NewsWidget",
  entryFields: { headline: "string", source: "string" },
  placeholder: { headline: "Loading...", source: "---" },
  // ...
});
```

## Family-Specific Rendering

The render function accepts an optional second parameter for the widget family, allowing different layouts per size:

```text
render: (entry, family) =>
  family === "systemLarge"
    ? VStack([
        Text(entry.title).font("title"),
        ForEach(entry.items, (item) => Text(item.name)),
      ])
    : HStack([
        Image("star.fill"),
        Text(entry.title).font("headline"),
      ]),
```

Supported families: `"systemSmall"`, `"systemMedium"`, `"systemLarge"`, `"accessoryCircular"`, `"accessoryRectangular"`, `"accessoryInline"`.

## App Group

The `appGroup` field specifies a shared container for data exchange between the host app and the widget:

```text
Widget({
  kind: "AppDataWidget",
  appGroup: "group.com.example.myapp",
  // ...
});
```

## Multiple Widgets

Define multiple widgets in a single file. They're bundled into a `WidgetBundle`:

```text
Widget({
  kind: "SmallWidget",
  // ...
});

Widget({
  kind: "LargeWidget",
  // ...
});
```

## Next Steps

- [Components](components.md) — Available widget components and modifiers
- [Overview](overview.md) — Widget system overview
