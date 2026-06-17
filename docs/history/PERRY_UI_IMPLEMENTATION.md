# Perry UI Implementation Plan

## Goal

Add native UI support to Perry. A developer writes TypeScript with a SwiftUI-inspired declarative syntax, and Perry compiles it to a native application that uses real platform widgets — no custom rendering, no browser, no Electron.

**End-to-end demo target:** A single `.ts` file compiles to a native macOS app that opens a window with a text label and a button. The same source code should be compilable to other platforms in the future without changes.

---

## Architecture Overview

Perry UI is split into **three layers**:

1. **`perry-ui`** — Platform-agnostic widget definitions, layout system, state management, and the public API. This crate knows nothing about macOS, iOS, or Linux. It defines *what* a Button is, not *how* to render it.

2. **`perry-ui-platform` crates** — One per platform. Each implements the widget traits from `perry-ui` using native APIs:
   - `perry-ui-macos` — AppKit (NSWindow, NSButton, NSTextField, etc.)
   - `perry-ui-ios` — UIKit (UIWindow, UIButton, UILabel, etc.)
   - `perry-ui-linux` — GTK4
   - `perry-ui-windows` — WinUI 3 / Win32 (future)
   - `perry-ui-android` — Android Views via JNI (future)

3. **Compiler integration** — `perry-codegen` learns to emit native API calls when it encounters `perry/ui` imports. The `--target` flag determines which platform crate is linked.

```
Developer writes:
  import { App, VStack, Text, Button } from "perry/ui"

Compiler sees --target macos-arm64:
  → Links perry-ui + perry-ui-macos
  → Generates objc_msgSend calls for AppKit

Compiler sees --target linux-x86_64:
  → Links perry-ui + perry-ui-linux  
  → Generates GTK4 function calls

Compiler sees --target ios-arm64:
  → Links perry-ui + perry-ui-ios
  → Generates UIKit calls + iOS app bundle
```

---

## Step 1: Create Crate Structure

Add to the workspace `Cargo.toml`:

```toml
[workspace]
members = [
    # ... existing crates ...
    "crates/perry-ui",
    "crates/perry-ui-macos",
    "crates/perry-ui-ios",
    "crates/perry-ui-linux",
]
```

### crates/perry-ui/

This is the **platform-agnostic core**. It defines:

#### src/lib.rs — Public API re-exports
```rust
pub mod widgets;
pub mod layout;
pub mod state;
pub mod app;
pub mod platform;
```

#### src/widgets.rs — Widget trait and enum definitions

Define a `Widget` trait that all widgets implement:

```rust
pub trait Widget {
    fn widget_type(&self) -> WidgetType;
    fn children(&self) -> &[Box<dyn Widget>];
    fn layout_params(&self) -> &LayoutParams;
}
```

Define concrete widget descriptors (these are data, not renderers):

```rust
pub enum WidgetType {
    // Layout
    VStack,
    HStack,
    ZStack,
    Spacer,
    Divider,
    ScrollView,
    // Content
    Text,
    Image,
    // Controls
    Button,
    TextField,
    Toggle,
    Slider,
    Picker,
    // Navigation
    NavigationStack,
    Sheet,
    Alert,
}
```

Each widget type has an associated config struct:

```rust
pub struct TextConfig {
    pub content: String,
    pub font: FontDescriptor,
    pub color: Color,
    pub alignment: TextAlignment,
}

pub struct ButtonConfig {
    pub label: String,
    pub style: ButtonStyle,
    pub on_press: CallbackId,  // Reference to compiled callback function
}

pub struct VStackConfig {
    pub spacing: f64,
    pub alignment: HorizontalAlignment,
    pub children: Vec<WidgetNode>,
}
```

#### src/layout.rs — Platform-agnostic layout constraints

NOT CSS. A simple constraint system:

```rust
pub struct LayoutParams {
    pub padding: EdgeInsets,
    pub frame: FrameConstraint,  // min/max/ideal width+height
    pub alignment: Alignment,
}

pub struct EdgeInsets {
    pub top: f64,
    pub leading: f64,
    pub bottom: f64,
    pub trailing: f64,
}

pub enum FrameConstraint {
    Fixed(f64, f64),
    Flexible { min_width: Option<f64>, max_width: Option<f64>, min_height: Option<f64>, max_height: Option<f64> },
    FillParent,
}
```

Layout resolution is simple: VStack/HStack distribute space among children linearly. No CSS cascade, no specificity, no multi-pass. Single pass, top-down.

#### src/state.rs — Reactive state management

Define the state model (SwiftUI-inspired):

```rust
pub struct StateVar<T> {
    pub id: StateId,
    pub initial_value: T,
}

pub struct Binding<T> {
    pub source: StateId,
    pub _phantom: std::marker::PhantomData<T>,
}
```

When state changes, the platform backend receives a notification to re-render the affected widget subtree. The mechanism:
1. State change triggers a `StateChange { id: StateId, new_value: Value }` event
2. The widget tree is diffed (which widgets depend on this StateId?)
3. Only affected native widgets are updated

#### src/platform.rs — Platform backend trait

This is the **bridge** that platform crates implement:

```rust
pub trait PlatformBackend {
    fn create_window(&mut self, config: &WindowConfig) -> WindowId;
    fn create_widget(&mut self, parent: WindowId, widget: &WidgetNode) -> NativeWidgetId;
    fn update_widget(&mut self, id: NativeWidgetId, widget: &WidgetNode);
    fn destroy_widget(&mut self, id: NativeWidgetId);
    fn run_event_loop(&mut self);  // Blocks until app exits
    fn set_callback(&mut self, id: CallbackId, callback: NativeCallback);
}

pub struct WindowConfig {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub resizable: bool,
}
```

#### src/app.rs — App lifecycle

```rust
pub struct AppConfig {
    pub name: String,
    pub root_widget: WidgetNode,
    pub window: WindowConfig,
}
```

---

### crates/perry-ui-macos/

Implements `PlatformBackend` using AppKit via Objective-C FFI.

**Dependency:** Use the `objc2` crate ecosystem:
```toml
[dependencies]
perry-ui = { path = "../perry-ui" }
objc2 = "0.6"
objc2-foundation = { version = "0.3", features = ["NSString", "NSArray", "NSThread"] }
objc2-app-kit = { version = "0.3", features = [
    "NSApplication", "NSWindow", "NSView", "NSButton", 
    "NSTextField", "NSStackView", "NSControl", "NSText",
    "NSRunningApplication", "NSResponder"
] }
```

#### Key files:

- **src/lib.rs** — `MacOSBackend` implementing `PlatformBackend`
- **src/window.rs** — NSWindow creation and management
- **src/widgets/mod.rs** — Widget creation dispatch
- **src/widgets/text.rs** — NSTextField (label mode) creation
- **src/widgets/button.rs** — NSButton creation with target-action
- **src/widgets/vstack.rs** — NSStackView with vertical orientation
- **src/widgets/hstack.rs** — NSStackView with horizontal orientation
- **src/event_loop.rs** — NSApplication run loop

#### Widget mapping (macOS):

| Perry Widget | AppKit Class | Notes |
|---|---|---|
| Text | NSTextField (non-editable) | `.isBezeled = false, .isEditable = false` |
| Button | NSButton | `.bezelStyle = .rounded` |
| TextField | NSTextField (editable) | Default editable text field |
| Toggle | NSSwitch | macOS 10.15+ |
| Slider | NSSlider | Continuous by default |
| VStack | NSStackView | `.orientation = .vertical` |
| HStack | NSStackView | `.orientation = .horizontal` |
| ScrollView | NSScrollView | With document view |
| Image | NSImageView | Load from file or URL |
| Divider | NSBox | `.boxType = .separator` |

---

### crates/perry-ui-ios/

Implements `PlatformBackend` using UIKit.

**Dependency:**
```toml
[dependencies]
perry-ui = { path = "../perry-ui" }
objc2 = "0.6"
objc2-foundation = { version = "0.3" }
objc2-ui-kit = { version = "0.3", features = [
    "UIApplication", "UIWindow", "UIView", "UIButton",
    "UILabel", "UITextField", "UIStackView", "UISwitch",
    "UISlider", "UIScrollView", "UIImageView"
] }
```

#### Widget mapping (iOS):

| Perry Widget | UIKit Class | Notes |
|---|---|---|
| Text | UILabel | Multi-line by default |
| Button | UIButton | `.configuration = .filled()` (iOS 15+) |
| TextField | UITextField | With border style |
| Toggle | UISwitch | Standard iOS toggle |
| Slider | UISlider | Continuous |
| VStack | UIStackView | `.axis = .vertical` |
| HStack | UIStackView | `.axis = .horizontal` |
| ScrollView | UIScrollView | With content view |
| Image | UIImageView | With content mode |
| Divider | UIView | 1px height, separator color |

---

### crates/perry-ui-linux/

Implements `PlatformBackend` using GTK4.

**Dependency:**
```toml
[dependencies]
perry-ui = { path = "../perry-ui" }
gtk4 = "0.9"
```

#### Widget mapping (Linux):

| Perry Widget | GTK4 Class | Notes |
|---|---|---|
| Text | gtk4::Label | With markup support |
| Button | gtk4::Button | Standard button |
| TextField | gtk4::Entry | Single-line text input |
| Toggle | gtk4::Switch | Standard toggle |
| Slider | gtk4::Scale | Horizontal scale |
| VStack | gtk4::Box | `Orientation::Vertical` |
| HStack | gtk4::Box | `Orientation::Horizontal` |
| ScrollView | gtk4::ScrolledWindow | Automatic scroll policies |
| Image | gtk4::Picture | From file or paintable |
| Divider | gtk4::Separator | Horizontal separator |

---

## Step 2: Compiler Integration

### TypeScript API (what the developer writes)

```typescript
import { App, VStack, HStack, Text, Button, State, TextField, Spacer } from "perry/ui"

function CounterApp() {
    const count = State(0)
    
    return App({
        title: "My Counter",
        width: 400,
        height: 300,
        body: VStack({ spacing: 16 }, [
            Text(`Count: ${count.value}`, { font: "title" }),
            HStack({ spacing: 8 }, [
                Button("Decrement", { onPress: () => count.set(count.value - 1) }),
                Button("Increment", { onPress: () => count.set(count.value + 1) }),
            ]),
        ])
    })
}

CounterApp()
```

### HIR Changes (crates/perry-hir/)

When the compiler sees `import ... from "perry/ui"`, it needs to:

1. **Recognize perry/ui as a built-in module** (like `fs`, `path`, `crypto` are today)
2. **Lower widget constructor calls to HIR nodes** — `VStack(...)` becomes an `HIR::WidgetCreate` node
3. **Lower State() to reactive state HIR nodes** — tracks dependencies

Add to `ir.rs`:
```rust
pub enum Expr {
    // ... existing variants ...
    WidgetCreate {
        widget_type: WidgetType,
        config: Box<Expr>,       // The config object
        children: Vec<Expr>,     // Child widgets
    },
    AppCreate {
        config: Box<Expr>,
    },
    StateCreate {
        initial_value: Box<Expr>,
    },
    StateGet {
        state_id: Box<Expr>,
    },
    StateSet {
        state_id: Box<Expr>,
        new_value: Box<Expr>,
    },
}
```

### Codegen Changes (crates/perry-codegen/)

For `--target macos-arm64`:

`WidgetCreate { widget_type: Button, ... }` generates:
```
call perry_ui_macos::create_button(label_ptr, label_len, callback_ptr) -> widget_id
```

For `--target linux-x86_64`:

Same `WidgetCreate` generates:
```
call perry_ui_linux::create_button(label_ptr, label_len, callback_ptr) -> widget_id
```

The codegen doesn't need to know about AppKit or GTK — it just calls the platform crate's exported functions. The platform crate handles the native API calls.

### CLI Changes (crates/perry/)

Extend the `--target` flag:

```
perry build app.ts --target macos-arm64     # macOS Apple Silicon
perry build app.ts --target macos-x86_64    # macOS Intel
perry build app.ts --target ios-arm64       # iOS (produces .app bundle)
perry build app.ts --target linux-x86_64    # Linux (requires GTK4)
perry build app.ts --target linux-arm64     # Linux ARM
perry build app.ts --target windows-x86_64  # Windows (future)
```

When `perry/ui` is imported and `--target` is set, the compiler links the appropriate platform crate.

**If perry/ui is imported but no --target is specified**, default to the host platform.

---

## Step 3: Tree-Shaking

Perry must only include what is used. This works at two levels:

### Cargo Feature Flags

In `perry-ui/Cargo.toml`:
```toml
[features]
default = []
text = []
button = []
textfield = []
toggle = []
slider = []
vstack = []
hstack = []
zstack = []
scrollview = []
image = []
navigation = []
all-widgets = ["text", "button", "textfield", "toggle", "slider", "vstack", "hstack", "zstack", "scrollview", "image", "navigation"]
```

Each platform crate mirrors these features:
```toml
# perry-ui-macos/Cargo.toml
[features]
text = ["perry-ui/text"]
button = ["perry-ui/button"]
# ...
```

### Compile-Time Widget Selection

When `perry-codegen` processes the source, it tracks which widgets are actually used:
- `import { Text, Button, VStack } from "perry/ui"` → only `text`, `button`, `vstack` features enabled
- Unused widgets are not compiled, not linked, not in the binary

The compiler should emit a build manifest that specifies exactly which widget features to enable, then invoke `cargo build` with those features.

---

## Step 4: Implementation Order

### Phase 1 — Foundation (do this first)

1. Create `crates/perry-ui/` with the trait definitions and widget types. No platform code yet. Just the API contract.

2. Create `crates/perry-ui-macos/` with a minimal `PlatformBackend` implementation:
   - `create_window` → opens an NSWindow
   - `create_widget` for `Text` → creates an NSTextField label
   - `create_widget` for `Button` → creates an NSButton
   - `create_widget` for `VStack` → creates an NSStackView
   - `run_event_loop` → runs NSApplication

3. Add a **standalone Rust test** (not through the Perry compiler yet) that:
   - Creates a `MacOSBackend`
   - Creates a window with a VStack containing a Text and a Button
   - Runs the event loop
   - **Verifies it works natively before touching the compiler**

4. Wire it into `perry-codegen`:
   - Recognize `perry/ui` imports
   - Generate calls to the platform backend
   - Link the correct platform crate based on `--target`

5. **Demo:** `perry build counter.ts --target macos-arm64` produces a native macOS app.

### Phase 2 — More Widgets + Linux

6. Add remaining widgets to macOS backend: TextField, Toggle, Slider, HStack, ScrollView, Image, Divider, Spacer.

7. Create `crates/perry-ui-linux/` with GTK4 backend implementing the same widgets.

8. **Demo:** Same `counter.ts` compiles to both macOS and Linux native apps.

### Phase 3 — State Management

9. Implement the reactive state system in `perry-ui`:
   - `State<T>` creation and tracking
   - Dependency graph (which widgets read which state?)
   - Change notification to platform backend

10. Wire state into codegen:
    - `State(0)` becomes a state allocation
    - `count.value` becomes a state read with dependency registration
    - `count.set(...)` becomes a state write + re-render trigger

11. **Demo:** Counter app with working increment/decrement that updates the UI.

### Phase 4 — iOS

12. Create `crates/perry-ui-ios/` with UIKit backend.

13. Add iOS app bundle generation to the compiler (Info.plist, code signing, etc.).

14. **Demo:** Same `counter.ts` compiles to an iOS app.

### Phase 5 — Navigation + Polish

15. NavigationStack, Sheet, Alert widgets.
16. Theming / Dark Mode support (read system preference, propagate to widgets).
17. Accessibility attributes (labels, hints — mapped to native accessibility APIs).

---

## Key Design Decisions

### Why native widgets, not custom rendering?

- **Performance:** Native widgets are GPU-accelerated by the OS. We don't beat Apple at rendering on their own hardware.
- **Feel:** Native scrolling, animations, dark mode, accessibility — all free.
- **Binary size:** No Skia dependency (15-20MB saved).
- **Maintenance:** OS updates improve our apps automatically.
- **Accessibility:** VoiceOver, TalkBack, screen readers work out of the box.

### Why SwiftUI-inspired, not React-inspired?

- SwiftUI's declarative model maps cleanly to TypeScript (object configs, closures).
- No JSX needed, no build step beyond Perry's compiler.
- State management is simpler (no useEffect, no dependency arrays).
- Property-based API is more TypeScript-idiomatic than JSX.

### Why separate platform crates?

- **Compile time:** Only the target platform is compiled.
- **Dependencies:** macOS crate pulls in objc2, Linux pulls in gtk4. Never mixed.
- **Maintainability:** Platform experts can work on one crate without touching others.
- **Future extensibility:** Adding Android = adding one crate, no changes to perry-ui.

---

## Important Constraints

1. **No runtime platform detection.** The platform is chosen at compile time via `--target`. The binary contains only one platform backend.

2. **No custom rendering fallback.** If a widget doesn't have a native mapping on a platform, it's a compile error, not a runtime fallback to canvas drawing.

3. **Widget API must be platform-agnostic.** No `NSButton`-specific properties in the TypeScript API. If a property can't be mapped to all supported platforms, it goes in a platform-specific extension, not the core API.

4. **State updates must be minimal.** When `count.set(5)` is called, only the `Text` widget displaying `count` should update. No full tree re-render.

5. **perry/ui is optional.** Server applications that don't import `perry/ui` must have zero overhead — no UI code linked, no platform dependencies.

---

## File Checklist

When this implementation is complete, the following files should exist:

```
crates/
├── perry-ui/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── widgets.rs
│       ├── layout.rs
│       ├── state.rs
│       ├── app.rs
│       └── platform.rs
│
├── perry-ui-macos/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── window.rs
│       ├── event_loop.rs
│       └── widgets/
│           ├── mod.rs
│           ├── text.rs
│           ├── button.rs
│           ├── vstack.rs
│           ├── hstack.rs
│           ├── textfield.rs
│           ├── toggle.rs
│           ├── slider.rs
│           ├── scrollview.rs
│           ├── image.rs
│           ├── divider.rs
│           └── spacer.rs
│
├── perry-ui-ios/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── window.rs
│       ├── event_loop.rs
│       └── widgets/
│           ├── mod.rs
│           ├── text.rs
│           ├── button.rs
│           ├── vstack.rs
│           ├── hstack.rs
│           └── ... (same as macos)
│
├── perry-ui-linux/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── window.rs
│       ├── event_loop.rs
│       └── widgets/
│           ├── mod.rs
│           ├── text.rs
│           ├── button.rs
│           ├── vstack.rs
│           ├── hstack.rs
│           └── ... (same as macos)
│
├── perry-hir/src/ir.rs         # Extended with Widget/State HIR nodes
├── perry-codegen/src/codegen.rs # Extended with UI codegen
└── perry/src/main.rs            # Extended --target flag

example-code/
└── counter-app/
    └── main.ts                  # The demo counter app

test-files/
├── test_ui_text.ts
├── test_ui_button.ts
├── test_ui_vstack.ts
└── test_ui_state.ts
```

---

## Testing Strategy

### Unit Tests (Rust)
- `perry-ui`: Test layout constraint resolution, state dependency tracking, widget tree diffing
- `perry-ui-macos`: Test that widget creation produces valid AppKit objects (requires macOS CI)
- `perry-ui-linux`: Test that widget creation produces valid GTK4 objects (requires Linux CI)

### Integration Tests (TypeScript → Binary)
- Compile `test_ui_text.ts` → verify binary runs and creates a window (use accessibility APIs or screenshot comparison)
- Compile `test_ui_button.ts` → verify button click triggers callback
- Compile same source for macOS and Linux → verify both produce working apps

### Parity Tests
- Every widget that works on macOS must also work on Linux (and vice versa)
- Track parity in a test matrix, similar to existing `test-parity/reports/`
