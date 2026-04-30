# Windows 7 Compatibility

Perry supports compiling executables that run on Windows 7 SP1 (and Windows 8 / 8.1) — opt-in via the `--min-windows-version` flag. The default target stays Windows 10+ to preserve full DPI fidelity and modern OS integration; legacy support is one flag away when you need it.

This page covers what works, what degrades, what's outright impossible, and how to validate your build before shipping.

## TL;DR

```bash
perry compile app.ts -o app.exe --target windows --min-windows-version=7
```

Produces a PE marked Win7-compatible. Perry's UI runtime resolves the Win10-only DPI APIs lazily at startup and falls back through Win8.1 → Vista primitives, so the binary starts on Win7 SP1. Most UI widgets work. Some cosmetic effects (rounded corners, dark titlebar) silently no-op. **No JavaScript-module imports allowed** on Win7 — the V8 runtime is Win10+ unconditional.

## Why this is opt-in

Two things make a Win7-compatible PE different from a default Perry build:

1. **The PE subsystem version field.** Default Perry builds let the linker pick (currently `/SUBSYSTEM:WINDOWS` with no version, which marks the binary as needing Win8+). Win7 needs `/SUBSYSTEM:WINDOWS,5.1` or `/SUBSYSTEM:CONSOLE,5.1`. The `,5.1` suffix is the [PE subsystem ABI](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#optional-header-windows-specific-fields-image-only) declaration of "I claim to run on Windows NT 5.1 or higher" — the OS loader reads this field before deciding whether to load the binary.

2. **Win10-only API calls become runtime-resolved.** Perry's UI library calls `SetProcessDpiAwarenessContext` (Win10 1607) and `GetDpiForSystem` (Win10 1607) for per-monitor v2 DPI awareness. Hard-importing them via `extern "system"` would emit IAT entries that the OS resolves *before* `main()` runs — on Win7, the loader fails the process with "entry point not found in user32.dll" before any Rust code can run. With `--min-windows-version=7` (and on default builds too — the retrofit is unconditional), Perry resolves these symbols lazily via `LoadLibraryW + GetProcAddress` and falls back through:

   | Tier | API | Min Windows |
   | --- | --- | --- |
   | 1 | `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` | Windows 10 1607 |
   | 2 | `SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE)` | Windows 8.1 |
   | 3 | `SetProcessDPIAware()` | Windows Vista |

   System DPI lookup uses the same lazy pattern: `GetDpiForSystem` (Win10) → `GetDC + GetDeviceCaps(LOGPIXELSY)` (Win2000+).

The `--min-windows-version` flag controls only (1) — the PE marker. The lazy DPI resolution from (2) is always active because it costs essentially nothing and makes default builds more robust against being run on stripped-down Windows installs.

## Accepted values

| `--min-windows-version` | Subsystem suffix | Targets | Default? |
| --- | --- | --- | --- |
| `10` | (none — linker default) | Windows 10+ | yes |
| `8` | `,6.02` | Windows 8 / 8.1+ | no |
| `7` | `,5.1` | Windows 7 SP1+ | no |

Anything else is a hard error at compile time — typos like `--min-windows-version=11` fail loudly instead of silently behaving like the default.

## What works on Win7

The same audit that produced this feature found 12 KLOC of Win32 UI code in `perry-ui-windows` and 5 calls that touch Win10+ APIs. The 5 break down as 2 hard blockers (now lazy-resolved) and 3 cosmetic-effect calls that already failed soft and silently no-op on Win7. So the bulk of the UI surface — every standard widget — works on Win7 SP1:

- All layout containers (`VStack`, `HStack`, `ZStack`, `ScrollView`, `Spacer`, `Divider`)
- All input widgets (`Button`, `TextField`, `SecureField`, `Toggle`, `Slider`, `Picker`, `ProgressView`)
- `Text`, `Canvas`, `Image` (file + symbol)
- `Form` and `LazyVStack`
- File / folder open / save dialogs
- Clipboard access
- Audio (WASAPI is Vista+)
- Keyboard shortcuts, menus, toolbars
- Multi-window
- The full `perry-runtime` and `perry-stdlib` surface — `fs`, `http`, `crypto`, `child_process`, `Date`, `Buffer`, etc.

## What degrades silently

These behaviors target Win10 / Win11 features. On Win7 the API call returns an error code that Perry already swallows; the binary runs, but the visual effect is missing:

- **DPI quality.** Win7 has system-wide DPI only — moving a window between monitors with different DPI doesn't trigger re-scaling. Per-monitor v2 (font hinting, dialog scaling) is Win10 1607+.
- **Dark titlebar.** `DWMWA_USE_IMMERSIVE_DARK_MODE` is Win10 1809+. Titlebar follows the system theme on Win7 (light only on stock Win7).
- **Rounded window corners.** `DWMWA_WINDOW_CORNER_PREFERENCE` is Win11+. Frameless windows have square corners on Win7 / Win10.
- **Mica / Acrylic backdrop.** `DWMWA_SYSTEMBACKDROP_TYPE` is Win11+. Backdrop falls through to the standard window background on Win7 / Win10.

## What is impossible

**`perry/jsruntime` (V8 / deno_core) is Win10+ unconditional.** Anything in your project that imports a `.js` module from `node_modules` triggers `--enable-jsruntime`, which links against deno_core, which embeds V8, which won't load on Win7. There's no fallback for this — Win7 builds must avoid JS-module imports entirely. If your project compiles cleanly without `--enable-jsruntime` (i.e. only TypeScript imports, only Perry-native packages), you're good.

**Universal Windows Platform (UWP / WinRT) APIs.** Perry doesn't currently use these, but if a future feature does (e.g. modern toast notifications), it'll be Win10+ only. The runtime + stdlib audit was clean as of v0.5.395.

## How the lazy DPI resolution works

The retrofit lives in `crates/perry-ui-windows/src/dpi_compat.rs`. It exposes two functions, both safe to call on any Windows version from Vista onward:

```rust,no-test
pub fn set_process_dpi_awareness_compat();
pub fn get_system_dpi_compat() -> u32;
```

Internally each function:

1. Calls `LoadLibraryA("user32.dll")` (and `shcore.dll` for the Win8.1 tier) — both are loaded into every Win32 process by the kernel before `main`, so the call is a cheap handle lookup.
2. Calls `GetProcAddress` to find the desired symbol. Caches the result (success or failure) in an `AtomicPtr` + `AtomicU8` pair, so the lookup runs at most once per process.
3. Falls through to the next tier on miss. `set_process_dpi_awareness_compat` ends with `SetProcessDPIAware()` (Vista+, hard-imported because every supported Windows has it). `get_system_dpi_compat` ends with `GetDeviceCaps(LOGPIXELSY)` (Win2000+, dead reliable).

After the cache is warm — i.e. after `app_create` runs once — every subsequent DPI query is a single atomic load + indirect call. No measurable runtime cost vs. a hard-imported call.

## Validating a Win7 build

Perry's CI and dev hosts don't have Win7 VMs. If you ship to Win7 you need to validate the binary yourself. Three checks:

### 1. PE subsystem version

Use `dumpbin /headers app.exe | findstr "subsystem"` (MSVC) or `objdump -p app.exe | grep "MajorOSVersion"` (LLVM):

```text
$ dumpbin /headers app.exe | findstr "subsystem"
            5.01 subsystem version
               2 subsystem (Windows GUI)
```

The `5.01` confirms the PE is Win7-compatible. A default build shows `6.00` or higher.

### 2. Imports

Use `dumpbin /imports app.exe | findstr /i "user32"` (MSVC) or `objdump -p app.exe | grep -A20 "DLL Name: user32"` (LLVM). Confirm that `SetProcessDpiAwarenessContext` and `GetDpiForSystem` are **not** in the user32.dll import list. If they are, the lazy retrofit isn't taking effect — likely you've added a `use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;` somewhere that pulls the symbol back in.

### 3. Run on a Win7 SP1 VM

There's no substitute for actually launching the binary. Microsoft's free Win7 evaluation VM (no longer hosted directly by Microsoft, mirrored on archive.org) is the canonical reference image. Worth keeping a snapshot for regression checks.

## Caveats and gotchas

- **The MSVC linker may warn** about subsystem version `,5.1` being below the C runtime's stated minimum on newer toolchains. The warning is benign — the CRT itself runs on Win7, the warning is conservative. Watch for hard errors, not warnings.
- **xwin sysroot setup is unchanged.** Cross-compiling from macOS / Linux still uses the `perry setup windows` xwin'd toolchain. Nothing in `--min-windows-version` changes the SDK requirements.
- **Static-link the CRT** if you want the binary to run on a clean Win7 SP1 install with no Visual C++ Redistributable. Confirm the binary doesn't import `vcruntime140.dll` / `msvcp140.dll` via the dumpbin/objdump check above.
- **`perry/thread`'s SRWLOCK is Vista+, fine.** Perry's threading primitives use Rust std, which uses SRWLOCK on Windows since Rust 1.42. No `WaitOnAddress` (Win8+) involvement on the supported Rust versions.

## Issue tracking

This feature lands as the resolution to [#303](https://github.com/PerryTS/perry/issues/303). If you hit a Win7-specific failure that isn't covered here, please file a follow-up referencing this page so we can extend the audit.
