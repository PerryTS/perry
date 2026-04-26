# Widget Components & Modifiers

Available components and modifiers for widgets.

## Text

```typescript,no-test
Text("Hello, World!")
Text(`${entry.name}: ${entry.value}`)
```

### Text Modifiers

Pass modifiers as a second argument options object — method-chain modifier syntax (e.g. `.font(...)`) produces a compile error:

```typescript,no-test
Text("Styled", { font: "title" })          // .title, .headline, .body, .caption, etc.
Text("Styled", { color: "blue" })          // Named color or hex
Text("Styled", { bold: true })
Text("Styled", { font: "title", color: "blue", bold: true })  // combined
```

## Layout

### VStack

```typescript,no-test
VStack([
  Text("Top"),
  Text("Bottom"),
])
```

### HStack

```typescript,no-test
HStack([
  Text("Left"),
  Spacer(),
  Text("Right"),
])
```

### ZStack

```typescript,no-test
ZStack([
  Image("background"),
  Text("Overlay"),
])
```

## Spacer

Flexible space that expands to fill available room:

```typescript,no-test
HStack([
  Text("Left"),
  Spacer(),
  Text("Right"),
])
```

## Image

Display SF Symbols or asset images:

```typescript,no-test
Image("star.fill")           // SF Symbol
Image("cloud.sun.rain.fill") // SF Symbol
```

## ForEach

Iterate over array entry fields to render a list of components:

```typescript,no-test
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

```typescript,no-test
VStack([
  Text("Above"),
  Divider(),
  Text("Below"),
])
```

## Label

A label with text and an SF Symbol icon:

```typescript,no-test
Label("Downloads", "arrow.down.circle")
Label(`${entry.count} items`, "folder.fill")
```

## Gauge

A circular or linear progress indicator:

```typescript,no-test
Gauge(entry.progress, 0, 100)       // value, min, max
Gauge(entry.battery, 0, 1.0)
```

## Modifiers

Widget components support SwiftUI-style modifiers:

### Font

```typescript,no-test
Text("Title").font("title")
Text("Body").font("body")
Text("Caption").font("caption")
```

### Color

```typescript,no-test
Text("Red text").color("red")
Text("Custom").color("#FF6600")
```

### Padding

```typescript,no-test
VStack([...]).padding(16)
```

### Frame

```typescript,no-test
widget.frame(width, height)
```

### Max Width

```typescript,no-test
widget.maxWidth("infinity")   // Expand to fill available width
```

### Minimum Scale Factor

Allow text to shrink to fit:

```typescript,no-test
Text("Long text").minimumScaleFactor(0.5)
```

### Container Background

Set background color for the widget container:

```typescript,no-test
VStack([...]).containerBackground("blue")
```

### Widget URL

Make the widget tappable with a deep link:

```typescript,no-test
VStack([...]).url("myapp://detail/123")
```

### Edge-Specific Padding

Apply padding to specific edges:

```typescript,no-test
VStack([...]).paddingEdge("top", 8)
VStack([...]).paddingEdge("horizontal", 16)
```

## Conditionals

Render different components based on entry data:

```typescript,no-test
render: (entry) =>
  VStack([
    entry.isOnline
      ? Text("Online").color("green")
      : Text("Offline").color("red"),
  ]),
```

## Complete Example

```typescript,no-test
import { Widget, Text, VStack, HStack, Image, Spacer } from "perry/widget";

Widget({
  kind: "StatsWidget",
  displayName: "Stats",
  description: "Shows daily stats",
  entryFields: {
    steps: "number",
    calories: "number",
    distance: "string",
  },
  render: (entry) =>
    VStack([
      HStack([
        Image("figure.walk"),
        Text("Daily Stats").font("headline"),
      ]),
      Spacer(),
      HStack([
        VStack([
          Text(`${entry.steps}`).font("title").bold(),
          Text("steps").font("caption").color("gray"),
        ]),
        Spacer(),
        VStack([
          Text(`${entry.calories}`).font("title").bold(),
          Text("cal").font("caption").color("gray"),
        ]),
        Spacer(),
        VStack([
          Text(entry.distance).font("title").bold(),
          Text("km").font("caption").color("gray"),
        ]),
      ]),
    ]).padding(16),
});
```

## Next Steps

- [Creating Widgets](creating-widgets.md) — Widget() API
- [Overview](overview.md) — Widget system overview
