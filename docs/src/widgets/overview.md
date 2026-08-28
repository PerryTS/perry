# Widgets (WidgetKit) Overview

Perry can compile TypeScript widget declarations to native widget extensions across 4 platforms: iOS (WidgetKit), Android (App Widgets), watchOS (Complications), and Wear OS (Tiles).

> **Status:** the `perry/widget` API is wired in the HIR
> (`crates/perry-hir/src/lower.rs:try_lower_widget_decl`) and emits via
> dedicated codegen crates (`perry-codegen-glance`, `perry-codegen-wear-tiles`,
> the WidgetKit emitter). The snippets on the widget docs pages compile-link
> cleanly on the host LLVM target — `Widget({...})` lowers to a no-op there —
> and CI verifies that via [`docs/examples/widgets/snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/widgets/snippets.ts).
> The doc-tests harness accepts a `// widget-bundle-id:` banner and passes it
> through as `--app-bundle-id`; that plumbing closed
> [#194](https://github.com/PerryTS/perry/issues/194). The shared snippets on
> these pages currently exercise the host compile/link path, while
> `_smoketest_bundleid.ts` exercises the banner. A target is compiled
> end-to-end only on a CI host where its Xcode or Android SDK is available.
> For a working end-to-end reference see [`examples/widget_demo.ts`](https://github.com/PerryTS/perry/blob/main/examples/widget_demo.ts).

## What Are Widgets?

Home screen widgets display glanceable information outside your app. Perry's `perry/widget` module lets you define widgets in TypeScript that compile to each platform's native widget system.

```typescript
{{#include ../../examples/widgets/snippets.ts:minimal}}
```

## How It Works

```
TypeScript widget declaration
    ↓ Parse & Lower to WidgetDecl HIR
    ↓ Platform-specific codegen
    ↓
iOS/watchOS: SwiftUI WidgetKit extension (Entry, View, TimelineProvider, WidgetBundle, Info.plist)
Android:    AppWidgetProvider + layout XML + AppWidgetProviderInfo
Wear OS:    TileService + layout
```

The compiler generates a complete native widget extension for each platform — no platform-specific language knowledge required.

## Building

```bash
perry widget.ts --target ios-widget              # iOS WidgetKit extension
perry widget.ts --target android-widget           # Android App Widget
perry widget.ts --target watchos-widget            # watchOS Complication
perry widget.ts --target watchos-widget-simulator   # watchOS Simulator
perry widget.ts --target wearos-tile               # Wear OS Tile
```

Each target produces the appropriate native widget extension for that platform.

## Next Steps

- [Creating Widgets](creating-widgets.md) — Widget() API in detail
- [Components & Modifiers](components.md) — Available widget components
- [Configuration](configuration.md) — Widget configuration options
- [Data Fetching](data-fetching.md) — Timeline providers and data loading
- [Cross-Platform Reference](platforms.md) — Platform-specific details
- [watchOS Complications](watchos.md) — watchOS-specific guide
- [Wear OS Tiles](wearos.md) — Wear OS-specific guide
