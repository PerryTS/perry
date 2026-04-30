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

```bash
perry compile app.ts -o app.exe --target windows
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

## Next Steps

- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
