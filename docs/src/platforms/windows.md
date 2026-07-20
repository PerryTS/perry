# Windows

Perry compiles TypeScript apps for Windows using the Win32 API.

## Requirements

- Windows 10 or later by default (Windows 7 SP1 / Windows 8 supported via `--min-windows-version=7|8` — see [Windows 7 Compatibility](./windows-7.md) for the trade-offs)
- A linker toolchain — either of these two options:

### Option A — Lightweight (recommended, ~1.5 GB, no Visual Studio)

Uses LLVM's `clang` + `lld-link` plus an xwin'd copy of the Microsoft CRT + Windows SDK libraries. No admin rights, no Visual Studio install.

```powershell
winget install LLVM.LLVM
perry setup windows
```

`perry setup windows` downloads ~700 MB (unpacks to ~1.5 GB) at `%LOCALAPPDATA%\perry\windows-sdk` after prompting you to accept the Microsoft redistributable license. Pass `--accept-license` to skip the prompt in CI. Partial downloads resume safely on re-run.

### Option B — Visual Studio (~8 GB)

If you already have Visual Studio installed, add the C++ workload via the Visual Studio Installer → *Modify* → check **Desktop development with C++**. Or install standalone Build Tools:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override `
  "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Both options produce identical binaries — Perry picks Option A when the xwin'd sysroot is present, Option B otherwise. Run `perry doctor` to see which is active.

## Building

```powershell
perry compile app.ts -o app.exe --target windows
```

For a runnable starting point, see [`examples/windows_ui_demo.ts`](https://github.com/PerryTS/perry/blob/main/examples/windows_ui_demo.ts) — a small window exercising Text, Button, TextField, Slider, and a `setInterval` timer:

```powershell
perry examples/windows_ui_demo.ts -o windows_ui_demo
.\windows_ui_demo.exe
```

## UI Toolkit

Perry maps UI widgets to Win32 controls:

| Perry Widget | Win32 Class |
|-------------|------------|
| Text | Static HWND |
| Button | HWND Button |
| TextField | Edit HWND |
| SecureField | Edit (ES_PASSWORD) |
| Toggle | Checkbox |
| Slider | Trackbar (TRACKBAR_CLASSW) |
| Picker | ComboBox |
| ProgressView | PROGRESS_CLASSW |
| Image | GDI |
| VStack/HStack | Manual layout |
| ScrollView | WS_VSCROLL |
| Canvas | GDI drawing |
| Form/Section | GroupBox |

## Windows-Specific APIs

- **Menu bar**: HMENU / SetMenu
- **Dark mode**: Windows Registry detection
- **Preferences**: Windows Registry
- **Keychain**: CredWrite/CredRead/CredDelete (Windows Credential Manager)
- **Notifications**: Toast notifications
- **File dialogs**: IFileOpenDialog / IFileSaveDialog (COM)
- **Alerts**: MessageBoxW
- **Open URL**: ShellExecuteW

## Troubleshooting

### `LNK1181: cannot open input file 'user32.lib'`

The linker couldn't find the Windows SDK libraries. Perry probes the registry (`KitsRoot10`) and the standard `Windows Kits\10\Lib\<ver>\um\x64` install paths; when the probe fails it prints a warning listing the paths it tried. Fixes, in order of preference:

- Run `vcvars64.bat` before `perry compile` (it sets the `LIB` environment variable)
- Install the Windows 10/11 SDK via the Visual Studio Installer
- Set `LIB` manually to your SDK's `um\x64;ucrt\x64` directories

### `LNK1158: cannot run 'mt.exe'`

MSVC `link.exe` shells out to the Windows SDK's `mt.exe` to embed the UI visual-styles manifest, and `mt.exe` isn't normally on `PATH` outside a `vcvars64.bat` shell. Perry locates the SDK `bin` directory and puts it on the linker's `PATH` automatically (issue #6023), so you shouldn't see this error anymore. If `mt.exe` isn't installed anywhere, Perry skips the manifest embed with a warning instead of failing — common controls render in the classic (unthemed) look until you install the Windows 10/11 SDK. `lld-link` (Option A) never needs `mt.exe`.

### `--staticlib` / `--dylib` each need one tool from the *other* toolchain

The two toolchain options currently cover plain `.exe` builds equally, but the library output modes each depend on a tool the other option provides:

- `--staticlib` archives with MSVC `lib.exe`. Option A (LLVM + `perry setup windows`) alone doesn't include it — install the MSVC Build Tools (Option B).
- `--dylib` links with LLVM `lld-link`; MSVC `link.exe` can't produce Perry's plugin DLLs (it reports success without writing the DLL under `/FORCE:UNRESOLVED`). Option B alone doesn't include it — `winget install LLVM.LLVM`.

A fix removing this cross-dependency is in flight; until it lands, the workaround is to install the missing half.

### SmartScreen blocks a downloaded `perry.exe`

Release binaries are not code-signed, so a `perry.exe` downloaded from GitHub Releases triggers "Windows protected your PC". Click **More info** → **Run anyway**. Installing via `winget` is generally less noisy than a raw download.

### Link fails with `os error 32` or `os error 5`

A previous build of your app is still running and holds a lock on the output `.exe`, so the linker can't overwrite it. Close the app (or `taskkill /IM app.exe /F`) and re-run the compile.

## Next Steps

- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
