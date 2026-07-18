# Wear OS Tiles

Perry widgets can compile to Wear OS Tiles using `--target wearos-tile`. Tiles are glanceable surfaces in the Wear OS tile carousel and watch face complications.

For full **Wear OS apps** (not glanceable Tiles), see [Wear OS](../platforms/wearos.md).

> **Status:** the snippet on this page compile-links cleanly on the host LLVM
> target via [`docs/examples/widgets/snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/widgets/snippets.ts), so the
> `Widget({...})` shape is verified against the codegen.
> `--target wearos-tile` itself is wired through `crates/perry-codegen-wear-tiles`
> but the doc-tests harness can't drive that cross-target yet —
> `--app-bundle-id` plumbing is still pending ([#194](https://github.com/PerryTS/perry/issues/194))
> and you'll need an Android NDK + Wear OS Gradle deps. Build with
> the `perry` CLI to validate end-to-end.

## Concepts

- **Tiles** are full-screen cards users swipe through on their watch
- **Complications** are small data displays on the watch face
- Perry compiles `Widget({...})` to a `SuspendingTileService` with layout builders

## Supported Components

| Widget API | Wear OS Mapping |
|-----------|----------------|
| `Text` | `LayoutElementBuilders.Text` |
| `VStack` | `LayoutElementBuilders.Column` |
| `HStack` | `LayoutElementBuilders.Row` |
| `Spacer` | `LayoutElementBuilders.Spacer` |
| `Divider` | Spacer with 1dp height |
| `Gauge(circular)` | `LayoutElementBuilders.Arc` + `ArcLine` |
| `Gauge(linear)` | Text fallback |
| `Image` | Resource-based (provide drawable) |

## Example

```typescript
{{#include ../../examples/widgets/snippets.ts:wearos-tile}}
```

## Compilation

```bash
perry widget.ts --target wearos-tile --app-bundle-id com.example.app -o tile_out
```

Output:
- `{Name}TileService.kt` — `SuspendingTileService` with tile layout
- `{Name}TileBridge.kt` — JNI bridge for native provider (if provider exists)
- `AndroidManifest_snippet.xml` — Service declaration

## Gradle Integration

Add to your Wear OS module's `build.gradle`:

```groovy
dependencies {
    implementation "com.google.android.horologist:horologist-tiles:0.6.5"
    implementation "androidx.wear.tiles:tiles-material:1.4.0"
    implementation "androidx.wear.tiles:tiles:1.4.0"
}
```

Merge the manifest snippet into your `AndroidManifest.xml`:

```xml
<service
    android:name=".StepsTileService"
    android:exported="true"
    android:permission="com.google.android.wearable.permission.BIND_TILE_PROVIDER">
    <intent-filter>
        <action android:name="androidx.wear.tiles.action.BIND_TILE_PROVIDER" />
    </intent-filter>
</service>
```

## Native Provider

Same as Android phone widgets — Wear OS is Android:
- Target triple: `aarch64-linux-android`
- `libwidget_provider.so` loaded via `System.loadLibrary`
- JNI bridge pattern identical to phone Glance widgets
- `sharedStorage()` uses `SharedPreferences`

## Refresh

Wear Tiles use `freshnessIntervalMillis` on the `Tile` builder. Set via a literal `reloadPolicy: { after: { minutes: N } }` in the provider return value — the interval is read at compile time. Default: 60 minutes. Minimum: 15 minutes (shorter requests are clamped with a compile-time warning). See [Reload Policies](data-fetching.md#reload-policies).
