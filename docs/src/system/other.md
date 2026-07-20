# Other System APIs

Additional platform-level APIs. Every snippet below is excerpted from a real
file CI compiles on every PR — see
[`docs/examples/system/snippets.ts`](../../examples/system/snippets.ts) for
the perry/system pieces and
[`docs/examples/ui/events/snippets.ts`](../../examples/ui/events/snippets.ts)
for clipboard.

## Open URL

Open a URL in the default browser or application:

```typescript
{{#include ../../examples/system/snippets.ts:open-url}}
```

| Platform | Implementation |
|----------|---------------|
| macOS | NSWorkspace.open |
| iOS | UIApplication.open |
| Android | Intent.ACTION_VIEW |
| Windows | ShellExecuteW |
| Linux | xdg-open |
| Web | window.open |

## Dark Mode Detection

```typescript
{{#include ../../examples/system/snippets.ts:dark-mode}}
```

| Platform | Detection |
|----------|-----------|
| macOS | NSApp.effectiveAppearance |
| iOS | UITraitCollection |
| Android | Configuration.uiMode |
| Windows | Registry (AppsUseLightTheme) |
| Linux | GTK settings |
| Web | prefers-color-scheme media query |

## Clipboard

Clipboard helpers live in `perry/ui` (not `perry/system`):

```typescript
{{#include ../../examples/ui/events/snippets.ts:clipboard}}
```

## Device Identity

```typescript
{{#include ../../examples/system/snippets.ts:device}}
```

`getDeviceIdiom()` returns the broad form factor as a string: `"phone"` or
`"pad"` on iOS (and `"phone"` on Android), `"mac"` on macOS, `"tv"` on tvOS,
`"watch"` on watchOS, `"vision"` on visionOS, and `"desktop"` on Windows and
Linux. `getDeviceModel()` returns the platform-specific model identifier
(`"iPhone15,2"`, `"MacBookPro18,3"`, etc. — on macOS this is sysctl
`hw.model`).

## Next Steps

- [Overview](overview.md) — All system APIs
- [UI Overview](../ui/overview.md) — Building UIs
