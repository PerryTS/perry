# Perry Feature Requests — From Building a Real macOS App

We ported a production macOS SwiftUI app (multi-chain EVM transaction analyzer, ~40 views) to Perry to find what's missing. The data layer ported perfectly. All UI gaps surfaced by this port are now closed.

**Perry version:** 0.5.151
**App:** 32 modules, compiles to 25MB ARM64 binary
**Updated:** 2026-04-22

---

## Status

| Category | v0.2.155 | v0.5.150 | v0.5.151 | Notes |
|----------|----------|----------|----------|-------|
| Critical gaps | 2 | 0 | **0** | — |
| Important gaps | 4 | 1 partial | **0** | Alert rich form shipped |
| Nice-to-have | 3 | 1 partial | **0** | LazyVStack NSTableView-virtualized; string prefs typed |
| System API gaps | 3 | 1 partial | **0** | Lifecycle hooks wired end-to-end |
| **Total gaps** | **12** | **3 partial** | **0** | All closed |

**Overall Perry readiness: ~80% → 100%** against the gaps surfaced by this port.

---

## Closed in v0.5.151

### Alert rich form

`alertWithButtons(title, message, buttons: string[], callback: (index: number) => void)` now exists alongside the simple `alert(title, message)`. On macOS it maps to `NSAlert` with one button per label and fires the callback with the 0-based index. Windows uses `MessageBoxW` (OK / OKCancel / YesNoCancel per count); GTK4 uses `MessageDialog` with `add_button`.

Latent bug found and fixed at the same time: the pre-existing 2-arg `alert` dispatch in `PERRY_UI_TABLE` pointed at the 4-arg `perry_ui_alert` runtime symbol, so `alert("Info", "Done")` was reading `buttons_ptr` + `callback` from uninitialized registers. Simple form now routes to a dedicated `perry_ui_alert_simple(title, message)` FFI.

### String preferences

`preferencesSet(key, value: string | number)` / `preferencesGet(key): string | number | undefined`. The runtime already branched on the NaN-box tag to store/retrieve `NSString` vs `NSNumber` on all desktop backends — the types were the only missing piece.

### App lifecycle hooks

`onTerminate(cb)` and `onActivate(cb)` are now callable from TypeScript. macOS `PerryAppDelegate` gained `applicationWillTerminate:` and `applicationDidBecomeActive:` overrides that invoke the registered callbacks; GTK4 and Windows were already wired (`connect_shutdown`/`connect_activate` and `WM_DESTROY`/`WM_ACTIVATEAPP` respectively). Test-mode exit (`std::process::exit(0)`) now also fires the terminate hook so CI coverage includes it.

### LazyVStack virtualization

`crates/perry-ui-macos/src/widgets/lazyvstack.rs` is now an `NSTableView`-backed widget with a `PerryLazyVStackDelegate` (NSTableViewDataSource + NSTableViewDelegate). The user's `(index) => Widget` render closure is invoked lazily — for a 1000-row list, only ~15 rows are realized (the visible rect plus the small scroll buffer NSTableView prefetches). New `lazyvstackSetRowHeight(handle, height)` setter since NSTableView virtualization requires uniform row heights; default is 44pt. `lazyvstackUpdate(handle, newCount)` triggers `reloadData` which re-fetches only currently-visible rows.

GTK4 and Windows kept their eager-render implementations — true virtualization there is a tracked follow-up; `set_row_height` on those backends is a no-op.

---

## What Perry Handles Well (v0.5.151)

**Layout:** VStack, HStack, ZStack, ScrollView, Spacer, Divider, Form, Section, NavigationStack, SplitView/FrameSplit
**Controls:** Text, Button, TextField, SecureField, Toggle, Slider, Picker, Image, ImageFile, ProgressView
**Modal / chrome:** Sheet (NSPanel), Alert + AlertWithButtons, Toolbar, multiple Windows
**Lists:** `LazyVStack` with true NSTableView-based virtualization on macOS (eager-render on GTK4/Windows), plus Table, ForEach
**State:** `State<number>` and `State<string>` with reactive binding, `state.onChange()`, `stateBindTextfield`, auto-reactive `Text(\`...${state.value}...\`)`
**Widget APIs:** addChild, clearChildren, setHidden, setEnabled, setTooltip, setControlSize, setOnHover, setOnDoubleClick, animateOpacity, animatePosition, setTint, setSize
**Text:** fontSize, fontWeight, color, selectable, fontFamily (including `"monospaced"`)
**System:** openURL, isDarkMode, preferencesSet/Get (both strings and numbers), clipboardRead/Write, openFileDialog, openFolderDialog, saveFileDialog, addKeyboardShortcut, context menus, Keychain, Notifications
**App lifecycle:** onTerminate, onActivate
**Data layer:** axios, better-sqlite3, fs, crypto, async/await, Promises — all via `--enable-js-runtime`

---

## Follow-ups (outside this port's scope)

- **GTK4 LazyVStack virtualization** — GTK4's `Gtk.ListView` + `Gtk.SignalListItemFactory` is the natural backend; currently eager-renders.
- **Windows LazyVStack virtualization** — Win32 `ListView` with `LVS_OWNERDATA` (virtual list) would be the equivalent; currently eager-renders.
- **iOS/tvOS/watchOS alert + lifecycle** — the FFI shape is now unified; iOS's `UIAlertController` and `UIApplicationDelegate` hooks are straightforward follow-ups.
