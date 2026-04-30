# Instructions: validate Perry's Win7 compatibility fix on Windows

> **You are running on a Windows 11 PowerShell session.** Your job is to verify that issue #303 (Perry executables refusing to start on Windows 7) is correctly resolved by the changes on branch `issue-303-windows-7`. The fix has two parts: (a) a `--min-windows-version=7` CLI flag that emits the Win7-compatible PE subsystem marker, and (b) lazy `LoadLibraryW + GetProcAddress` resolution of the Win10-only DPI APIs, replacing the static imports that caused Win7's loader to refuse the process before `main()` ran.
>
> **You don't need a Windows 7 machine to do most of this work.** Steps 1–5 give 80% confidence that the fix is correct via static PE analysis on Win11. Step 6 is optional and only relevant if a Win7 SP1 VM happens to be available.
>
> If anything in this document is unclear or doesn't match what you observe, **stop and report back** with the exact command output. Do not improvise around unexpected results — they're the most important data point.

---

## What "success" looks like at a glance

After running through this doc you'll have:

1. A built `perry.exe` from the `issue-303-windows-7` branch.
2. Two compiled test executables: `hello-win7.exe` (CLI) and `ui-win7.exe` (UI).
3. `dumpbin /headers` confirming both binaries declare PE subsystem version `5.01` (= Win7-compatible).
4. `dumpbin /imports` confirming neither binary lists `SetProcessDpiAwarenessContext` or `GetDpiForSystem` in its `user32.dll` import table — they're resolved at runtime instead.
5. Both binaries running cleanly on Win11 (sanity check — Win11 should be the easy case).

If all five pass, the fix is correct. If a Win7 VM is available, step 6 confirms end-to-end.

---

## Prerequisites

Confirm these exist (open PowerShell, paste each one, expect non-error output):

```powershell
git --version
rustc --version    # need 1.75+
cargo --version
```

For PE inspection, you need either Visual Studio Build Tools' `dumpbin.exe` (if you used Option B during `perry setup windows`) or LLVM's `llvm-objdump.exe` (if you used Option A — `winget install LLVM.LLVM`). **Either works.** Test:

```powershell
# Try dumpbin first (it's the standard MSVC tool)
dumpbin

# If that fails with "command not found", try LLVM
llvm-objdump --version
```

Note which one is available — examples below show `dumpbin` first; substitute the equivalent `llvm-objdump` form if needed (commands documented inline at each step).

If neither is available, install LLVM: `winget install LLVM.LLVM`. After install, open a fresh PowerShell so PATH picks up the new tool.

---

## Step 1 — Clone or update the repo to the Win7 branch

```powershell
# If you don't have a clone yet:
git clone https://github.com/PerryTS/perry.git
cd perry

# Or if you already have one:
cd perry
git fetch origin
git checkout issue-303-windows-7
git pull --ff-only
```

Confirm you're on the right branch:

```powershell
git branch --show-current
# Expected: issue-303-windows-7

git log -1 --oneline
# Expected first line should reference: feat(windows): #303 Win7 / Win8 compatibility
```

---

## Step 2 — Build perry from this branch

```powershell
cargo build --release -p perry-runtime -p perry-stdlib -p perry-ui-windows -p perry
```

Expected: builds clean. If you get **errors** (not warnings), stop and report them back verbatim. If you get the standard 40-ish unused-code warnings that the codebase carries, that's fine — keep going.

The compiled `perry.exe` lands at `target\release\perry.exe`. Confirm:

```powershell
.\target\release\perry.exe --version
```

Expected: prints a version string near `0.5.395`.

Confirm the new flag is wired in:

```powershell
.\target\release\perry.exe compile --help | Select-String "min-windows-version" -Context 0,5
```

Expected: shows `--min-windows-version <MIN_WINDOWS_VERSION>` and a few lines of doc text mentioning Win7 / Win8 / `,5.1` / `,6.02`. If this output is missing, the build picked up old code — stop and report back.

---

## Step 3 — Compile two test programs targeting Win7

Pick a clean directory anywhere outside the perry repo to avoid polluting the workspace:

```powershell
$test_dir = "$env:TEMP\perry-win7-test"
New-Item -ItemType Directory -Force -Path $test_dir | Out-Null
cd $test_dir

# CLI smoke — proves the runtime + stdlib link cleanly with --min-windows-version=7
'console.log("hello win7");' | Set-Content hello.ts

# UI smoke — proves the lazy DPI-resolution path works (this is the actual fix)
@"
App({
    title: "Win7 Smoke Test",
    body: VStack([
        Text("Issue #303"),
        Button("OK", () => {})
    ])
});
"@ | Set-Content ui.ts
```

Build each one with `--min-windows-version=7`:

```powershell
$perry = "C:\path\to\perry\target\release\perry.exe"  # ← adjust to your actual path
& $perry compile hello.ts -o hello-win7.exe --min-windows-version=7
& $perry compile ui.ts -o ui-win7.exe --min-windows-version=7
```

Expected: each prints a short progress summary and produces a `.exe` (typically 1-3 MB for `hello-win7.exe` and 4-6 MB for `ui-win7.exe`). If either fails, stop and report the error.

Also build the **default** (Win10+) variants for comparison — these should still work and produce different headers:

```powershell
& $perry compile hello.ts -o hello-default.exe
& $perry compile ui.ts -o ui-default.exe
```

---

## Step 4 — Static PE check 1: subsystem version

This is the headline check. The PE subsystem version field tells the OS loader the minimum Windows version the binary claims to support. Win7 = `5.01`, Win8 = `6.02`, default = whatever the linker picks (typically `6.00` or higher).

### Using `dumpbin` (MSVC):

```powershell
Write-Host "--- ui-win7.exe (expect 5.01 subsystem version) ---"
dumpbin /headers ui-win7.exe | Select-String -Pattern "subsystem version|subsystem \("

Write-Host "--- hello-win7.exe (expect 5.01 subsystem version) ---"
dumpbin /headers hello-win7.exe | Select-String -Pattern "subsystem version|subsystem \("

Write-Host "--- ui-default.exe (expect 6.00 or higher) ---"
dumpbin /headers ui-default.exe | Select-String -Pattern "subsystem version|subsystem \("
```

### Or using `llvm-objdump`:

```powershell
Write-Host "--- ui-win7.exe ---"
llvm-objdump -p ui-win7.exe | Select-String -Pattern "MajorOSVersion|MinorOSVersion|MajorSubsystemVersion|MinorSubsystemVersion"

Write-Host "--- hello-win7.exe ---"
llvm-objdump -p hello-win7.exe | Select-String -Pattern "MajorOSVersion|MinorOSVersion|MajorSubsystemVersion|MinorSubsystemVersion"
```

### Expected (dumpbin form):

```
--- ui-win7.exe (expect 5.01 subsystem version) ---
            5.01 subsystem version
               2 subsystem (Windows GUI)
--- hello-win7.exe (expect 5.01 subsystem version) ---
            5.01 subsystem version
               3 subsystem (Windows CUI)
--- ui-default.exe (expect 6.00 or higher) ---
            6.00 subsystem version
               2 subsystem (Windows GUI)
```

(The default may be `6.00` or `6.02` or even `10.00` depending on the linker — anything ≥ `6.00` is "not Win7" and proves the flag is doing something different.)

### What this tells us

- If both `*-win7.exe` files show **`5.01`**, the linker subsystem flag is being emitted correctly. ✅
- If they show `6.00` or higher, the `--min-windows-version=7` flag is being silently ignored — **stop and report**. The fix is broken at the linker layer.

---

## Step 5 — Static PE check 2: imports table

This is the more important check. The pre-fix code statically imported `SetProcessDpiAwarenessContext` and `GetDpiForSystem` from `user32.dll`. Those imports cause the Win7 loader to refuse the binary before `main()` runs. The fix replaces them with `LoadLibraryW + GetProcAddress`, so the **import table must not contain those names** anywhere — they're loaded lazily.

### Using `dumpbin`:

```powershell
Write-Host "--- ui-win7.exe imports (expect NO matches below) ---"
dumpbin /imports ui-win7.exe | Select-String -Pattern "SetProcessDpiAwarenessContext|GetDpiForSystem"

Write-Host "--- hello-win7.exe imports (expect NO matches below) ---"
dumpbin /imports hello-win7.exe | Select-String -Pattern "SetProcessDpiAwarenessContext|GetDpiForSystem"
```

### Or using `llvm-objdump`:

```powershell
Write-Host "--- ui-win7.exe imports ---"
llvm-objdump -p ui-win7.exe | Select-String -Pattern "SetProcessDpiAwarenessContext|GetDpiForSystem"

Write-Host "--- hello-win7.exe imports ---"
llvm-objdump -p hello-win7.exe | Select-String -Pattern "SetProcessDpiAwarenessContext|GetDpiForSystem"
```

### Expected

**No output** for either binary. The pattern shouldn't match.

Also confirm the lazy loaders ARE present (sanity check that user32 / gdi32 are still being imported, just for the harmless functions):

```powershell
Write-Host "--- ui-win7.exe should import LoadLibraryA, GetProcAddress, SetProcessDPIAware (Vista+, fine) ---"
dumpbin /imports ui-win7.exe | Select-String -Pattern "LoadLibraryA|GetProcAddress|SetProcessDPIAware|GetDeviceCaps"
```

Expected: shows hits for at least `LoadLibraryA`, `GetProcAddress`, and `GetDeviceCaps` (and ideally `SetProcessDPIAware`). All four are Vista+ exports that ship on every supported Windows.

### What this tells us

- If `SetProcessDpiAwarenessContext` / `GetDpiForSystem` are **absent** from the import table, the lazy retrofit is taking effect. ✅ **This is the most important check in the whole document.**
- If either name appears, somebody (you, a future change, or a transitive dependency) re-introduced a static import. The Win7 loader will refuse the binary. **Stop and report**.

---

## Step 6 — Optional: actually run on a Win7 VM

This step requires a Windows 7 SP1 VM. You can skip this if you don't have one — steps 4 and 5 give very high confidence on their own. If you do have a VM:

1. Copy `hello-win7.exe` and `ui-win7.exe` into the VM (shared folder, USB, network share, doesn't matter).
2. Open `cmd` or PowerShell in the VM and run `hello-win7.exe`. Expected: prints `hello win7` and exits cleanly. **Pre-fix this would die immediately with a dialog that says "the procedure entry point SetProcessDpiAwarenessContext could not be located in the dynamic link library user32.dll" — the binary never reaches `main()`.**
3. Double-click `ui-win7.exe`. Expected: a window opens with the title "Win7 Smoke Test" and shows the text "Issue #303" + an "OK" button. The window may look slightly different from Win11 (no rounded corners, different titlebar style, possibly fuzzier on hi-DPI displays — that's expected and documented in `docs/src/platforms/windows-7.md`).
4. Click the OK button — it should not crash, even though the click handler is empty.
5. Close the window — the process should exit cleanly.

If any of these fail, capture:
- The exact error message (screenshot if it's a dialog, copy-paste if it's text)
- The Windows 7 build number (run `winver` in the VM)
- The output of `dumpbin` from steps 4 & 5 (run on Win11 against the same .exe files)

---

## Step 7 — Run on Win11 too (sanity)

The fix should not regress Win11 behavior. Run both `*-win7.exe` and `*-default.exe` on the Win11 machine and confirm all four work:

```powershell
.\hello-win7.exe
.\hello-default.exe
.\ui-win7.exe
.\ui-default.exe
```

Expected: all four behave identically. The `*-win7.exe` versions may render windows slightly less crisp on hi-DPI displays compared to `*-default.exe` (the lazy DPI path can fall through one extra tier on some Win11 configurations) but otherwise function identically. UI windows should open cleanly, accept clicks, and close without crashing.

---

## What to report back

A single message containing:

1. **Step 4 output** — the literal `dumpbin` / `llvm-objdump` output for all three (or four) binaries, copy-pasted.
2. **Step 5 output** — confirmation that the negative grep produced no matches, plus the LoadLibrary / GetProcAddress / SetProcessDPIAware import confirmation.
3. **Step 7 results** — "all four ran cleanly on Win11" or specifics if anything misbehaved.
4. **Step 6 results if applicable** — Win7 VM build number + UI screenshot.
5. **Anything unexpected** — error messages, crashes, build warnings that look related, anything that seemed off. Better too much info than too little.

If steps 4 and 5 both pass, mark this as ✅ ready-to-merge. If either fails, mark as ❌ and include the failing output.

---

## Cleanup (optional)

After validating:

```powershell
Remove-Item -Recurse -Force $env:TEMP\perry-win7-test
```

The branch can stay checked out — there's no follow-up work pending. Once it's merged into `main`, the Windows-side bits will be exercised by every future build automatically.
