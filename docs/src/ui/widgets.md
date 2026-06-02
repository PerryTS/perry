# Widgets

Perry provides native widgets that map to each platform's native controls.
Every example on this page is a real runnable program verified by CI
(`scripts/run_doc_tests.sh`) — the snippet you read is the same source that's
compiled and launched.

The widget API is **free functions**, not methods. A widget is a 64-bit
opaque handle; you pass it into helpers like `textSetFontSize(widget, 18)`
rather than calling `widget.setFontSize(18)`. That's the only shape perry/ui
supports — no fluent chain, no prototype methods.

## Text

Displays read-only text.

```typescript,no-test
{{#include ../../examples/ui/widgets/text.ts}}
```

Color is RGBA with each channel in `[0.0, 1.0]` — divide a hex byte by 255
(`0x33 / 255 ≈ 0.2`).

**Helpers:** `textSetString`, `textSetFontSize`, `textSetFontWeight`,
`textSetFontFamily`, `textSetColor`, `textSetWraps`, `textSetSelectable`.

Text widgets inside template literals with `state.value` update automatically
— perry detects the state read and rewires the widget to re-render on change.
See [State Management](state.md).

## Button

A clickable button.

```typescript,no-test
{{#include ../../examples/ui/widgets/button.ts}}
```

**Helpers:** `buttonSetTitle`, `buttonSetBordered`, `buttonSetImage`
(SF Symbol name on macOS/iOS), `buttonSetImagePosition`,
`buttonSetContentTintColor`, `buttonSetTextColor`, `widgetSetEnabled`.

## TextField

An editable single-line text input.

```typescript,no-test
{{#include ../../examples/ui/widgets/textfield.ts}}
```

`TextField(placeholder, onChange)` fires `onChange` as the user types. Pair
with `stateBindTextfield(state, field)` for two-way binding so programmatic
`state.set(…)` also updates the visible text.

**Helpers:** `textfieldSetString`, `textfieldSetFontSize`,
`textfieldSetTextColor`, `textfieldSetBackgroundColor`,
`textfieldSetBorderless`, `textfieldSetOnSubmit`, `textfieldSetOnFocus`,
`textfieldSetNextKeyView`.

## SecureField

A password input — identical signature to `TextField`, but text is masked.

```typescript,no-test
{{#include ../../examples/ui/widgets/secure_field.ts}}
```

## Toggle

A boolean on/off switch.

```typescript,no-test
{{#include ../../examples/ui/widgets/toggle.ts}}
```

## Slider

A numeric slider.

```typescript,no-test
{{#include ../../examples/ui/widgets/slider.ts}}
```

`Slider(min, max, onChange)` — `onChange` fires on every drag. Use
`stateBindSlider(state, slider)` for two-way binding.

## Picker

A dropdown selection control. Items are added with `pickerAddItem`.

```typescript,no-test
{{#include ../../examples/ui/widgets/picker.ts}}
```

## ImageFile / ImageSymbol

Two distinct constructors:

- `ImageFile(path)` — image from a file path
- `ImageSymbol(name)` — SF Symbol glyph name (macOS/iOS only)

```typescript,no-test
{{#include ../../examples/ui/widgets/image_symbol.ts}}
```

Use `widgetSetWidth(img, N)` / `widgetSetHeight(img, N)` to size the image.

## ProgressView

An indeterminate or determinate progress indicator.

```typescript,no-test
{{#include ../../examples/ui/widgets/progressview.ts}}
```

## TextArea

A multi-line text input. Same `(placeholder, onChange)` signature as
`TextField` but renders as a multi-line box.

```typescript,no-test
{{#include ../../examples/ui/widgets/textarea.ts}}
```

**Helpers:** `textareaSetString`.

## Sections

Group controls into labelled sections. Perry has no `Form()` widget — use a
`VStack` of `Section(title)`s and attach children via `widgetAddChild`.

```typescript,no-test
{{#include ../../examples/ui/widgets/sections.ts}}
```

## Mobile widgets (issue #553)

### BottomNavigation

5-tab bottom bar with icon + label + badge per tab. `onSelect(index)`
fires when the user taps; `bottomNavSetSelected` is the programmatic
counterpart and does NOT fire `onSelect`.

```typescript,no-test
import {
  BottomNavigation,
  bottomNavAddItem,
  bottomNavSetBadge,
} from "perry/ui";

const bar = BottomNavigation((index) => {
  console.log("tab:", index);
});
bottomNavAddItem(bar, "house", "Home");
bottomNavAddItem(bar, "magnifyingglass", "Search");
bottomNavAddItem(bar, "bell", "Activity");
bottomNavSetBadge(bar, 2, "5");
```

Real on macOS (`NSStackView` + `NSButton` strip with SF Symbol icons),
iOS (`UITabBar`), Android (custom `LinearLayout` strip with badge
overlay), and GTK4 (`GtkBox` + Adwaita CSS). Stub on Windows, tvOS,
visionOS, watchOS.

### ImageGallery

Swipeable, paging carousel of images. Local file paths load
synchronously; HTTP/HTTPS URLs are fetched on a background queue and
applied on the main thread.

```typescript,no-test
import { ImageGallery, imageGalleryAddImage } from "perry/ui";

const gallery = ImageGallery((idx) => console.log("page:", idx));
imageGalleryAddImage(gallery, "/photos/01.jpg", "Hero shot");
imageGalleryAddImage(gallery, "https://cdn.example/photo2.jpg", "Wide angle");
```

Real on macOS (`NSScrollView` paging), iOS (`UIScrollView` with
`scrollViewDidEndDecelerating`), Android (`HorizontalScrollView`), GTK4
(`GtkScrolledWindow` + `GtkPicture`). Stub on Windows, tvOS, visionOS,
watchOS.

### Pull-to-refresh

Available on `ScrollView` and `LazyVStack`. The `onPull` callback fires
once when the user pulls past the threshold; call
`scrollviewEndRefreshing` (or `lazyvstackEndRefreshing`) when your async
fetch settles to dismiss the spinner.

```typescript,no-test
import {
  ScrollView,
  scrollviewSetRefreshControl,
  scrollviewEndRefreshing,
} from "perry/ui";

const scroll = ScrollView();
scrollviewSetRefreshControl(scroll, async () => {
  await refreshFeed();
  scrollviewEndRefreshing(scroll);
});
```

Real on iOS (`UIRefreshControl`). The macOS / Android / GTK4 / Windows
desktops have no native pull-to-refresh idiom — they're documented
no-ops.

### Infinite scroll (`onScrollEnd`)

Fires once when the user scrolls past `thresholdPx` (or `thresholdItems`
for `LazyVStack`) from the bottom; re-arms after the user scrolls back
up past the threshold so a single fetch is queued at a time.

```typescript,no-test
import { ScrollView, scrollviewSetScrollEndCallback } from "perry/ui";

const scroll = ScrollView();
scrollviewSetScrollEndCallback(
  scroll,
  () => loadMore(),
  200, // threshold in pixels from the bottom
);
```

Real on every platform that has a scroll view: macOS
(`NSViewBoundsDidChangeNotification`), iOS
(`UIScrollViewDelegate.scrollViewDidScroll`), Android
(`View.OnScrollChangeListener`), GTK4 (`GtkAdjustment::value-changed`),
Windows (`WM_VSCROLL` / `WM_MOUSEWHEEL`).

## Platform-specific widgets

These exist only on specific platforms and aren't verified by the
cross-platform doc-tests:

- **`Table(rows, cols, renderer)`** — macOS only. Now supports
  `tableSetSortColumn`, `tableSetFilterText`, and multi-select since
  v0.5.636 (#473).
- **`QRCode(data, size)`** — macOS only. Renders a QR code.
- **`Canvas(width, height, draw)`** — all desktop platforms. A drawing
  surface; see [Canvas](canvas.md).
- **`CameraView()`** — iOS only (other platforms planned). See
  [Camera](camera.md).

### Combobox (issue #475)

Editable text field with a filterable dropdown of suggestions. macOS
uses `NSComboBox` with as-you-type completion; other platforms stub the
FFI today (the field falls back to a plain editable field).

```typescript,no-test
import { Combobox, comboboxAddItem, comboboxGetValue } from "perry/ui";

const combo = Combobox("", (v) => console.log("picked:", v));
comboboxAddItem(combo, "apple");
comboboxAddItem(combo, "apricot");
comboboxAddItem(combo, "avocado");
```

### TreeView / Outline (issue #480)

Hierarchical disclosure list. Build the topology bottom-up via `TreeNode`
+ `treeNodeAddChild`, then mount it via `TreeView`. macOS uses
`NSOutlineView`; other platforms stub.

```typescript,no-test
import { TreeNode, treeNodeAddChild, TreeView } from "perry/ui";

const dox = TreeNode("docs", "Documents");
treeNodeAddChild(dox, TreeNode("doc-1", "Resume.pdf"));
treeNodeAddChild(dox, TreeNode("doc-2", "Cover Letter.pdf"));
const root = TreeNode("root", "Files");
treeNodeAddChild(root, dox);
const tree = TreeView(root, (id) => console.log("selected:", id));
```

### Calendar (issue #481)

Month-grid date picker. macOS uses `NSDatePicker` in graphical style;
other platforms stub. `onChange` receives the selected date as an ISO
`yyyy-MM-dd` string.

```typescript,no-test
import { Calendar, calendarGetSelectedDate } from "perry/ui";

const cal = Calendar(2026, 5, (iso) => console.log("date:", iso));
```

### Chart (issue #474)

Line / bar / pie via CoreGraphics on macOS. `kind` is `0=line`, `1=bar`,
`2=pie`. Apple Charts framework / SwiftUI Charts integration on iOS 16+
is a follow-up.

```typescript,no-test
import { Chart, chartAddDataPoint, chartSetTitle } from "perry/ui";

const chart = Chart(0, 600, 400);
chartSetTitle(chart, "Visits");
chartAddDataPoint(chart, "Mon", 12);
chartAddDataPoint(chart, "Tue", 18);
chartAddDataPoint(chart, "Wed", 9);
```

### Command palette (issue #477)

⌘K-style fuzzy command launcher. macOS shows a floating `NSPanel`; other
platforms stub. Bind `commandPaletteShow()` to ⌘K via
`addKeyboardShortcut` to wire the default hotkey.

```typescript,no-test
import {
  commandPaletteRegister,
  commandPaletteShow,
} from "perry/ui";

commandPaletteRegister("save", "Save", "⌘S", () => save());
commandPaletteRegister("export", "Export PDF", "", () => exportPdf());
// then:
commandPaletteShow();
```

### MapView (issue #517)

Wraps `MKMapView` on macOS / iOS / visionOS / tvOS, `libshumate` on GTK4,
Google Maps SDK on Android (requires API key in
`AndroidManifest.xml`), and the SwiftUI `Map` view on watchOS. Windows
remains a stub (WinUI MapControl needs XAML Islands integration).

```typescript,no-test
import {
  MapView,
  mapViewSetRegion,
  mapViewAddPin,
  mapViewSetMapType,
} from "perry/ui";

const map = MapView(800, 600);
mapViewSetRegion(map, 37.7749, -122.4194, 0.05, 0.05);
mapViewAddPin(map, 37.7749, -122.4194, "San Francisco");
mapViewSetMapType(map, 1); // 0=standard, 1=satellite, 2=hybrid
```

### PdfView (issue #516)

`PDFView` from PDFKit on macOS / iOS / visionOS. `pdfViewLoadFile`
returns 1 on success, 0 on failure.

```typescript,no-test
import {
  PdfView,
  pdfViewLoadFile,
  pdfViewGetPageCount,
} from "perry/ui";

const pdf = PdfView(800, 600);
if (pdfViewLoadFile(pdf, "/tmp/report.pdf")) {
  console.log("pages:", pdfViewGetPageCount(pdf));
}
```

### RichTextEditor (issue #478)

`NSTextView` with `NSAttributedString` storage on macOS. Plain-text and
HTML round-trip cover persistence; `richTextToggleBold` /
`ToggleItalic` / `ToggleUnderline` cover inline formatting via
`NSResponder` actions.

```typescript,no-test
import {
  RichTextEditor,
  richTextSetHtml,
  richTextGetHtml,
  richTextToggleBold,
} from "perry/ui";

const editor = RichTextEditor(600, 400, (text) => console.log(text));
richTextSetHtml(editor, "<p>Hello <b>world</b></p>");
richTextToggleBold(editor);
```

### Rich tooltip (issue #479)

`widgetSetRichTooltip(widget, content, hoverDelayMs)` — like
`widgetSetTooltip` but the tooltip content is itself a Perry widget.
macOS uses `NSPanel` + `NSTrackingArea`; other platforms stub. For
plain-text tooltips with VoiceOver / a11y support, prefer the simpler
`widgetSetTooltip`.

### WebView (issue #658)

`WebView({ url, allowedDomains?, onShouldNavigate?, ... })` embeds a
real browser engine — `WKWebView` on Apple, `WebView2` on Windows,
`WebKitGTK 6.0` on Linux, `android.webkit.WebView` on Android,
sandboxed `<iframe>` on web. See [WebView](webview.md) for the full
OAuth / callback-interception pattern and the per-platform notes.

These are linked from their own pages where richer examples exist.

## Common widget helpers

Every widget handle accepts these:

| Helper | Description |
|---|---|
| `widgetSetWidth(w, n)` / `widgetSetHeight(w, n)` | Explicit size in points |
| `widgetSetBackgroundColor(w, r, g, b, a)` | RGBA in [0, 1] |
| `setCornerRadius(w, r)` | Rounded corners in points |
| `widgetSetOpacity(w, alpha)` | Opacity in [0, 1] |
| `widgetSetEnabled(w, flag)` | `0` disables, `1` enables |
| `widgetSetHidden(w, flag)` | `0` visible, `1` hidden |
| `widgetSetTooltip(w, text)` | Tooltip on hover (desktop only) |
| `widgetSetOnClick(w, cb)` | Click handler |
| `widgetSetOnHover(w, cb)` | Hover enter/leave (desktop only) |
| `widgetSetOnDoubleClick(w, cb)` | Double-click handler |
| `widgetSetEdgeInsets(w, top, left, bottom, right)` | Padding around contents |
| `widgetSetBorderColor(w, r, g, b, a)` / `widgetSetBorderWidth(w, n)` | Border |
| `widgetAddChild(parent, child)` | Attach a child to a container |
| `widgetSetContextMenu(w, menu)` | Right-click menu |

See [Styling](styling.md) and [Events](events.md) for deeper coverage.

## Next Steps

- [Layout](layout.md) — Arranging widgets with stacks and containers
- [Styling](styling.md) — Colors, fonts, borders
- [State Management](state.md) — Reactive bindings
