# Provider Function and Data Fetching

The `provider` function is the heart of a dynamic widget. It fetches data, transforms it, and returns timeline entries that the system renders on schedule.

> **Status:** the basic `WeatherWidget` provider below compile-links cleanly on
> the host LLVM target via
> [`docs/examples/widgets/snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/widgets/snippets.ts),
> so the `provider`/`reloadPolicy`/`entryFields` shapes are verified against
> the codegen. The shorter fragments lower on the page (a bare
> `reloadPolicy:`, a `provider:` body without surrounding `Widget({...})`,
> etc.) are rendered as plain text. The `sharedStorage()` and
> `preferencesSet()` examples are also rendered as plain text — those symbols
> are provided by the platform-specific glue (`AppGroupBridge.swift`,
> `Bridge.kt`) for `--target ios-widget`/`android-widget`/`watchos-widget`/
> `wearos-tile` and don't link on the host LLVM target. The cross-compile
> targets themselves still aren't driven by the doc-tests harness — each
> needs `--app-bundle-id` and a platform SDK
> ([#194](https://github.com/PerryTS/perry/issues/194)).

## Provider Lifecycle

1. The system calls your provider when the widget is first added, when a snapshot is needed, and when the reload policy expires.
2. Your provider runs as native LLVM-compiled code linked into the widget extension.
3. The provider returns one or more timeline entries. The system renders each entry at its scheduled time.
4. After the last entry, the reload policy determines when the provider runs again.

## Basic Provider

```typescript
{{#include ../../examples/widgets/snippets.ts:weather-provider}}
```

## Authenticated Requests with Shared Storage

Widgets run in a separate process and cannot access your app's memory. Use `sharedStorage()` to read values that your app has written to a shared container.

### iOS / watchOS: App Groups

On Apple platforms, shared storage maps to `UserDefaults(suiteName:)` backed by an App Group container. Set the `appGroup` field in your widget declaration:

```text
Widget({
  kind: "DashboardWidget",
  displayName: "Dashboard",
  description: "Account summary",
  appGroup: "group.com.example.shared",

  entryFields: {
    revenue: "number",
    users: "number",
  },

  provider: async () => {
    const token = sharedStorage("auth_token");
    const res = await fetch("https://api.example.com/dashboard", {
      headers: { Authorization: `Bearer ${token}` },
    });
    const data = await res.json();
    return {
      entries: [{ revenue: data.revenue, users: data.activeUsers }],
      reloadPolicy: { after: { minutes: 30 } },
    };
  },

  render: (entry) =>
    VStack([
      Text(`$${entry.revenue}`, { font: "title" }),
      Text(`${entry.users} active users`, { font: "caption" }),
    ]),
});
```

Your main app writes the token to the shared container:

```text
import { preferencesSet } from "perry/system";
// In your app's login flow:
preferencesSet("auth_token", token);
```

**Setup requirement (iOS):** Add an App Group capability in Xcode to both the main app target and the widget extension target. The identifier must match the `appGroup` value.

### Android / Wear OS: SharedPreferences

On Android, shared storage maps to `SharedPreferences` with the name `perry_shared`. The generated `Bridge.kt` reads values via `context.getSharedPreferences("perry_shared", MODE_PRIVATE)`.

## Reload Policies

The `reloadPolicy` field controls when the system next calls your provider:

```text
return {
  entries: [{ ... }],
  reloadPolicy: { after: { minutes: 30 } },
};
```

The refresh interval is read **at compile time**: the compiler scans the
provider's `return` statements for a literal
`reloadPolicy: { after: { minutes: N } }` (where `N` is a numeric literal;
fractional values round to the nearest second) and bakes the interval into
the generated platform code. A `reloadPolicy` computed at runtime (a
variable, a function call, …) cannot be read — the compiler warns and the
platform default applies.

| Policy | Behavior |
|--------|----------|
| `{ after: { minutes: N } }` | Re-fetch after N minutes. Compiles to `.after(Date().addingTimeInterval(N*60))` on iOS/watchOS, `android:updatePeriodMillis="N*60000"` on Android, and `setFreshnessIntervalMillis(N*60000)` on Wear OS. |
| *(omitted)* | Platform default: 30 minutes on iOS/watchOS, 30 minutes on Android, 60 minutes on Wear OS. |

**Platform floors:** each platform ignores refresh requests below a minimum
interval, so the compiler clamps and warns:

| Platform | Default | Minimum (clamped) |
|----------|---------|-------------------|
| iOS / watchOS (WidgetKit) | 30 minutes | 15 minutes (refresh budget) |
| Android (Glance, `updatePeriodMillis`) | 30 minutes | 30 minutes (hard framework floor) |
| Wear OS (Tiles freshness) | 60 minutes | 15 minutes |

If different `return` statements carry different literal policies (e.g. a
short error-retry interval and a longer happy-path one), the smallest one
wins — the interval is a single compile-time constant per widget — and the
compiler emits a warning naming the value it chose.

**Budget limits:** iOS restricts widget refreshes. Typical budget is 40--70 refreshes per day. watchOS is stricter (see [watchOS Complications](watchos.md)). Request only what you need.

## JSON Response Handling

The provider function receives the parsed JSON directly. Entry field types must match your `entryFields` declaration:

```text
entryFields: {
  items: { type: "array", items: { type: "object", fields: { name: "string", count: "number" } } },
  total: "number",
},

provider: async () => {
  const res = await fetch("https://api.example.com/items");
  const data = await res.json();
  return {
    entries: [{
      items: data.results.map((r: any) => ({ name: r.name, count: r.count })),
      total: data.total,
    }],
  };
},
```

## Error Handling

If the fetch fails or JSON parsing throws, the widget extension falls back to the placeholder data:

```text
Widget({
  // ...
  placeholder: { temperature: 0, condition: "Loading..." },

  provider: async () => {
    const res = await fetch("https://api.example.com/weather");
    if (!res.ok) {
      // Return stale/fallback data with a short retry interval
      return {
        entries: [{ temperature: 0, condition: "Unavailable" }],
        reloadPolicy: { after: { minutes: 5 } },
      };
    }
    const data = await res.json();
    return {
      entries: [{ temperature: data.temp, condition: data.desc }],
      reloadPolicy: { after: { minutes: 15 } },
    };
  },
});
```

The `placeholder` field provides data shown in the widget gallery and during loading. If the provider throws an unhandled exception, the generated Swift/Kotlin code catches it and renders the placeholder instead.

Note that with two distinct literal policies (5 and 15 minutes above), the
compiled widget uses the smaller one — 5 minutes, which the platform floor
then raises to its minimum (15 minutes on iOS) — and the compiler warns
about the choice. See [Reload Policies](#reload-policies).

## Multiple Timeline Entries

Return multiple entries to schedule future content without re-fetching:

```text
provider: async () => {
  const res = await fetch("https://api.example.com/hourly");
  const hours = await res.json();
  return {
    entries: hours.map((h: any) => ({
      temperature: h.temp,
      condition: h.condition,
    })),
    reloadPolicy: { after: { minutes: 60 } },
  };
},
```

Each entry is rendered at the corresponding date in the timeline. The system transitions between entries automatically.

## Next Steps

- [Configuration](configuration.md) -- User-configurable parameters
- [Cross-Platform Reference](platforms.md) -- Build targets and platform differences
