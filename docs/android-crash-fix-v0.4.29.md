# Android Crash Fix — Perry v0.4.26–v0.4.29

> **Historical incident record.** This documents the v0.4.26–v0.4.29 Android
> crash and its fix; it is not current Android setup guidance. See
> [`src/platforms/android.md`](src/platforms/android.md) for the maintained
> platform documentation.

## Summary

Android UI apps crashed with `SIGSEGV at getLevelInfo+44` when accessing module-level arrays from UI pump tick callbacks. The root cause was the perry-native thread's arena being freed after `main()` returned.

## Root Cause

On Android, Perry's init chain runs on a background thread ("perry-native"):

```
Kotlin PerryActivity.startNative()
  → Thread { PerryBridge.nativeMain() }
    → main()              // compiled TypeScript entry point
      → _perry_init_labels_ts()   // initializes thresholds[], levels[]
      → App({ ... })              // sets up UI + timer pump
    → main() returns
    → thread exits              ← ARENA DROPPED HERE
```

After `main()` returns, the perry-native thread exits. Rust's thread-local storage cleanup runs the arena's `Drop`, which **frees all memory blocks** — including module-level arrays (`thresholds`, `levels`) that were allocated during init.

The UI thread's 8ms timer pump then calls `getLevelInfo()`, which dereferences the now-freed array pointers → **SIGSEGV**.

This never happened on macOS/iOS because `App()` blocks forever in the event loop — the thread never exits, so the arena is never dropped.

## Fix

**`crates/perry-ui-android/src/lib.rs`**: After `main()` returns, park the thread forever instead of letting it exit:

```rust
main();
// Park thread — arena holds module-level objects needed by UI thread
loop { std::thread::park(); }
```

## Additional Fixes in v0.4.26–v0.4.29

| Version | Fix |
|---------|-----|
| v0.4.26 | Skip `strip_duplicate_objects_from_lib` on Android — `js_nanbox_*` symbols were stripped from UI lib while standalone runtime was skipped |
| v0.4.27 | `JNI_GetCreatedJavaVMs` stub — `jni-sys` extern ref unsatisfied on Android (no `libjvm.so`, `libnativehelper` only at API 31+) |
| v0.4.28 | Module-level arrays with `Unknown`/`Any` type loaded as F64 instead of I64 — now arrays/closures/maps/sets always use I64 |
| v0.4.29 | Thread-local arena freed after `main()` returned + `-Bsymbolic` linker flag to prevent ELF symbol interposition |

## How to Rebuild for Android

```bash
# 1. Ensure perry v0.4.29+ is installed
perry --version  # should show 0.4.29

# 2. Cross-compile the Android UI library (one-time)
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/28.0.12433566 \
  cargo build --release -p perry-ui-android --target aarch64-linux-android

# 3. Compile your app
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/28.0.12433566 \
  perry src/main.ts --target android -o android-build/app/src/main/jniLibs/arm64-v8a/libperry_app.so

# 4. Clean Gradle build (important — clears cached .so)
cd android-build && ./gradlew clean assembleDebug
```
