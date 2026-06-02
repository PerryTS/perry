# HarmonyOS NEXT

Perry compiles TypeScript apps for HarmonyOS NEXT (Huawei's mobile OS) by emitting **declarative ArkUI** alongside a logic-only `.so` library. The same TypeScript source that targets macOS, iOS, Android, Linux, and Windows also runs natively on HarmonyOS — no platform-specific adapters needed in user code.

## Architecture

HarmonyOS NEXT runs apps via the ArkTS runtime, which owns the UI tree. Perry can't lower `perry/ui` calls to the imperative AppKit/UIKit/etc shape used on every other platform — it has to play by ArkTS's declarative rules. So the harmonyos target is structured differently:

```
TypeScript (.ts)
   ↓
HIR (perry-hir)
   ↓
perry-codegen-arkts (harvest pass)
   ├── walks App({body: ...}) call
   ├── extracts widget tree → emits pages/Index.ets (real ArkUI source)
   ├── captures closure args → registers slot ids
   ├── strips the App call from the HIR
   └── injects perry_arkts_register_callback() per closure
   ↓
perry-codegen (LLVM)
   ↓
libentry.so (no UI calls — just logic + NAPI bridge)
```

The user splices three artifacts into a DevEco Studio project — `libentry.so`, `pages/Index.ets`, `cpp/types/libentry/Index.d.ts` — and DevEco signs + runs as usual. Tap interactions, text input, etc. fire NAPI calls into the `.so`, which dispatch the registered Perry closure bodies.

## What's supported

**Widgets** (introduced in v0.5.401, expanded in v0.5.418, v0.5.429):

| Widget | ArkUI emission |
|---|---|
| `Text(content)` / `Text(content, "id")` | `Text(...).fontSize(20)` (reactive when id is given) |
| `VStack(children)` / `VStack(spacing, children)` | `Column({ space })` |
| `HStack(children)` / `HStack(spacing, children)` | `Row({ space })` |
| `Button(label, onPress)` | `Button(...).onClick(...)` |
| `TextField(placeholder, onChange)` | `TextInput(...).onChange(...)` |
| `Toggle(label, onChange)` | `Toggle({type: ToggleType.Switch}).onChange(...)` |
| `Slider(min, max, onChange)` | `Slider({...}).onChange(...)` |
| `Spacer()` | `Blank()` |
| `Divider()` | `Divider()` |
| `Image(src)` / `ImageFile(path)` | `Image(...)` |
| `ScrollView(children)` | `Scroll() { Column() { ... } }` |
| `LazyVStack(children)` | `Column({...})` (eager — see v10 follow-up) |
| `Picker(options, onChange)` | `TextPicker({...}).onChange(...)` |
| `ProgressView(value, total)` | `Progress({type: ProgressType.Linear})` |
| `Section(title, children)` | `Column({ space: 4 }) { Text(title) ... }` |

**Event handling** (v0.5.417 + v0.5.421):
- `Button.onPress` → `invokeCallback(idx)` via NAPI
- `Toggle.onChange((isOn: boolean) => ...)` → `invokeCallback1(idx, isOn)`
- `TextField.onChange((value: string) => ...)` → `invokeCallback1(idx, value)`
- `Slider.onChange((value: number) => ...)` → `invokeCallback1(idx, value)`
- `Picker.onChange((idx: number) => ...)` → `invokeCallback1(idx, index)`

**Reactivity** (v0.5.419 + v0.5.421):
- `Text("0", "counter")` registers a reactive slot bound to a generated `@State text_counter: string` field.
- `setText("counter", "5")` from inside any closure updates the Text on-screen.

**Toast banners** (v0.5.419):
- `showToast("Saved!")` from inside any closure shows an ArkUI `promptAction.showToast({ message })` banner.

**Inline styling** (v0.5.429):
- `Text("hi", { fontSize: 16, color: "red" })` maps to `.fontSize(16).fontColor('red')`.
- Supported props: `backgroundColor`, `color`, `fontSize`, `fontWeight`, `fontFamily`, `borderRadius`, `padding` (number or per-side object), `opacity`, `hidden`, `borderColor` + `borderWidth` (combined as `.border({...})`).
- PerryColor objects (`{r,g,b,a}`) auto-convert to `rgba(...)` strings.

**Dynamic lists** (v0.5.429):
- `VStack(items.map(item => Text(item)))` lowers to ArkUI `ForEach(items, (__item) => { Text(__item) }, (__item) => __item)`.
- Single-arg map closures only; complex array sources require Phase 2 v6 state binding.

## Setup

1. **Install DevEco Studio** + the OpenHarmony SDK from Huawei. Verified working with DevEco Studio 6.0.2 + OpenHarmony 5.0+.

2. **Run the setup wizard once** (introduced in v0.5.380):

   ```bash
   perry setup harmonyos
   ```

   The wizard auto-discovers your DevEco-generated debug certificates from `~/.ohos/config/`, prompts for the keystore password, and persists the configuration to `~/.perry/config.toml`. Subsequent `perry compile --target harmonyos` invocations sign HAPs automatically.

3. **Optional**: install `hdc` (HarmonyOS Device Connector) for emulator interaction. It ships inside DevEco at `Contents/sdk/default/openharmony/toolchains/hdc`.

## Compile + run workflow

Write a TypeScript program with `App({body: ...})`:

```typescript,no-test
// hi.ts
import { App, VStack, Text, Button, showToast } from "perry/ui";

let count = 0;

App({
  title: "Perry on HarmonyOS",
  body: VStack([
    Text("Count: 0", "counter"),
    Button("+", () => {
      count++;
      setText("counter", `Count: ${count}`);
    }),
    Button("Notify", () => {
      showToast(`Counter is ${count}`);
    }),
  ]),
});
```

Compile for HarmonyOS:

```bash
perry compile hi.ts --target harmonyos -o /tmp/libentry.so
```

This produces three artifacts in `/tmp/`:

- `libentry.so` — the compiled `.so` (8-9 MB typically)
- `ets/pages/Index.ets` — the auto-emitted ArkUI page
- `cpp/types/libentry/Index.d.ts` — the NAPI declaration file

**Splice** into a DevEco Studio project:

```bash
cp /tmp/libentry.so       ~/DevEcoStudioProjects/MyApp/entry/libs/arm64-v8a/libentry.so
cp /tmp/ets/pages/Index.ets   ~/DevEcoStudioProjects/MyApp/entry/src/main/ets/pages/Index.ets
cp /tmp/cpp/types/libentry/Index.d.ts   ~/DevEcoStudioProjects/MyApp/entry/src/main/cpp/types/libentry/Index.d.ts
```

Click ▶ Run in DevEco — DevEco's hvigor signs + bundles the HAP and installs onto the emulator (or attached device). The app launches, taps fire your TS closures, and the screen updates reactively.

## Architecture deep dive

### The harvest model

`perry-codegen-arkts::emit_index_ets` walks `module.init` looking for the first `App({body: <expr>})` call from `perry/ui`. It extracts the `body` field, recursively emits ArkUI source for each widget in the tree, and **destructively replaces the App call with `Stmt::Expr(Expr::Number(0.0))`** so the LLVM backend never sees `perry_ui_*` FFI calls (which would be unresolved on the OHOS target — there's no `perry-ui-harmonyos` crate by design).

The emitted Index.ets is a real ArkUI `@Entry @Component struct Index { build() { ... } }` page with `@State` declarations for any reactive Text widgets, `import promptAction from '@ohos.promptAction'` for toast routing, and per-Button onClick handlers that invoke NAPI callbacks then drain queued toasts and text updates.

### Closures across the NAPI boundary

Each Button/Toggle/etc onClick closure registers via `perry_arkts_register_callback(idx, closure_handle)` during `main()` startup. The `closure_handle` is a NaN-boxed pointer to a real Perry `*ClosureHeader`. A GC root scanner registered in `gc_init` keeps registered closures alive across collections.

When ArkUI fires an onClick, the auto-emitted `.onClick(() => perryEntry.invokeCallback(0))` calls back into the `.so` via NAPI. The `invoke_callback` NAPI handler in `crates/perry-runtime/src/ohos_napi.rs` reads the int32 idx, looks up the slot, and dispatches via `js_closure_call0`. Multi-arg variants (Toggle/TextField/Slider) use `invokeCallback1(idx, value)` with `napi_typeof` dispatch to NaN-box the value (boolean / string / number) before calling `js_closure_call1`.

### The drain queue pattern

`showToast` and `setText` calls inside a closure body push entries onto thread-local queues:
- `PENDING_TOASTS: Mutex<VecDeque<String>>`
- `PENDING_TEXT_UPDATES: Mutex<VecDeque<(String, String)>>`

After every onClick/onChange invocation, the auto-emitted handler in Index.ets drains both queues:

```ets
.onClick(() => {
    perryEntry.invokeCallback(0);
    let __t = perryEntry.drainToast();
    while (__t !== undefined) {
        promptAction.showToast({ message: __t });
        __t = perryEntry.drainToast();
    }
    let __u = perryEntry.drainTextUpdate();
    while (__u !== undefined) {
        this.applyTextUpdate(__u.id, __u.value);
        __u = perryEntry.drainTextUpdate();
    }
})
```

`applyTextUpdate(id, value)` is a switch over registered Text ids that assigns to the matching `@State text_<id>: string` field — ArkUI's reactivity then rerenders the Text widget.

### Why NAPI?

HarmonyOS NEXT uses the OpenHarmony NAPI binding (modeled on Node's NAPI) to load native `.so` libraries from ArkTS. Perry's `crates/perry-runtime/src/ohos_napi.rs` registers a module via `napi_module_register` in an `.init_array` constructor (Rust's equivalent of `__attribute__((constructor))`), with the modname auto-derived from the `.so` filename via `dladdr`. The exported NAPI surface is just `run` / `invokeCallback` / `invokeCallback1` / `drainToast` / `drainTextUpdate` — every other Perry runtime call happens within the `.so` itself.

## Known limitations

- **LazyVStack is currently rendered eagerly** as a plain `Column`. Real lazy rendering for big lists needs ArkUI's `LazyForEach` + a custom `IDataSource` impl — tracked as Phase 2 v10.
- **State binding is one-way** — `setText("id", value)` from a closure updates the Text on-screen, but a generic `state<T>` reactive container (`const count = state(0); count.set(...)`) is Phase 2 v6 follow-up work.
- **Multi-page navigation** (NavStack / Router across multiple `.ets` files) is Phase 2 v11.
- **AppGallery production signing** uses a different cert chain than DevEco's debug certs and isn't yet plumbed into `perry compile`. The current splice workflow handles debug-emulator deploy.
- **Real device validation** is pending — every milestone has been verified on the Pura 90 Pro Max emulator. AppGallery upload + real-hardware install will follow.

## Validated on emulator

End-to-end on Pura 90 Pro Max with a 5-widget interactive page (counter + reset, TextField echoing input live as `You typed: <text>`, Toggle flipping `Notifications: on/off` with toast feedback, Slider tracking `Volume: N` continuously, reactive Texts everywhere). Each interaction routes:

```
ArkUI event → invokeCallback{,1} → typeof-dispatch in NAPI → NaN-box marshal
            → js_closure_call{0,1} → user TS body runs with the typed arg
            → closure calls setText / showToast → drain queues → ArkUI rerenders
```

This is the first time Perry-compiled TypeScript state mutation has reactively driven a HarmonyOS NEXT screen.

## Version history

- **v0.5.401** — Phase 2 v1.5: full widget set rendering (Text/VStack/HStack/Button/TextField/Toggle/Slider/Spacer/Divider).
- **v0.5.417** — Phase 2 v2 + v3 + v2.5: Button onClick callback bridge, showToast, reactive Text via setText, multi-arg Toggle/TextField/Slider value forwarding.
- **v0.5.418** — Phase 2 v4: Image / ScrollView / LazyVStack / Picker / ProgressView / Section.
- **v0.5.420 / .421** — Cross-platform showToast + setText on iOS / tvOS / visionOS / Android.
- **v0.5.422 / .423** — Cross-platform showToast + setText on Windows / GTK4.
- **v0.5.429** — Phase 2 v5: inline `style: { ... }` + ForEach via array.map.

For the full per-version detail see [CHANGELOG.md](https://github.com/PerryTS/perry/blob/main/CHANGELOG.md).
