# App Store Review

> **Status: extension-dependent.** The compiler-side wiring (`--bundle-extensions`, `perry.nativeLibrary`, `declare function`) is in place — see [Native Extensions — Status](native-extensions.md) — but the snippets below assume the `perry-appstore-review` extension repo has been cloned into `./extensions/`. The doc-tests harness doesn't ship that repo, so these snippets are kept as `,no-test`. Once the extension is on disk, they compile and run on iOS / iOS Simulator / macOS / Android.

Prompt users to rate your app using the native app store review dialog on iOS and Android.

The `perry-appstore-review` extension exposes a single function — `requestReview()` — that opens the platform's native review prompt. It does nothing else: when and how often to ask is entirely up to you.

**Repository:** [github.com/PerryTS/appstorereview](https://github.com/PerryTS/appstorereview)

## Quick start

### 1. Add the extension

Clone or copy the extension into your project's extensions directory:

```bash
mkdir -p extensions
cd extensions
git clone https://github.com/PerryTS/appstorereview.git perry-appstore-review
cd ..
```

Your project structure:

```
my-app/
├── package.json
├── src/
│   └── index.ts
└── extensions/
    └── perry-appstore-review/
```

### 2. Use in your app

```text
import { requestReview } from "perry-appstore-review";

// Show the review prompt when the user completes a meaningful action
async function onLevelComplete() {
  await requestReview();
}
```

### 3. Build

```bash
perry src/index.ts -o app --target ios --bundle-extensions ./extensions
```

The `--bundle-extensions` flag tells Perry to discover, compile, and link all native extensions in the given directory. The app store review native code is compiled and statically linked into your binary — no runtime dependencies.

## API

### `requestReview(): Promise<void>`

Opens the native app store review prompt. Returns a promise that resolves when the prompt has been presented (or skipped by the OS).

```text
import { requestReview } from "perry-appstore-review";

await requestReview();
```

The function only triggers the prompt. It does not:
- Track whether the user has already reviewed
- Throttle how often the prompt appears (iOS does this automatically; Android does not)
- Return whether the user actually left a review (neither platform provides this)

## Platform behavior

### iOS

The current extension uses
[`SKStoreReviewController.requestReview(in:)`](https://developer.apple.com/documentation/storekit/skstorereviewcontroller/requestreview(in:))
for its broad deployment-target compatibility. Apple now deprecates that API
in favor of
[`AppStore.requestReview(in:)`](https://developer.apple.com/documentation/storekit/appstore/requestreview(in:)-1q8qs).
Treat the legacy call as a compatibility implementation detail; new extension
releases should move to `AppStore.requestReview(in:)` when their minimum OS
allows it.

| Detail | Value |
|--------|-------|
| Native API | Compatibility path: deprecated `SKStoreReviewController.requestReview(in:)`; preferred modern API: `AppStore.requestReview(in:)` |
| Minimum iOS version | 14.0 |
| Framework | StoreKit |
| Thread | Dispatched to main thread automatically |
| Throttling | Apple limits display to 3 times per 365-day period per app. The system may silently ignore the call. |
| Development builds | Always shown in debug/TestFlight builds |
| User control | Users can disable review prompts in Settings > App Store |

**Important:** Apple's throttling means the prompt is not guaranteed to appear every time `requestReview()` is called. Design your app flow so that not showing the prompt doesn't break the user experience.

### macOS

Uses the same StoreKit API. Shares the iOS native crate (both compile from `crate-ios`).

| Detail | Value |
|--------|-------|
| Native API | Compatibility path: deprecated `SKStoreReviewController.requestReview()`; prefer StoreKit's current `AppStore`/environment review action where available |
| Minimum macOS version | 13.0 |
| Framework | StoreKit |
| Throttling | Same as iOS — system-controlled |

Only works for apps distributed through the Mac App Store.

### Android

Uses the [Google Play In-App Review API](https://developer.android.com/guide/playcore/in-app-review).

| Detail | Value |
|--------|-------|
| Native API | `ReviewManager.requestReviewFlow()` + `launchReviewFlow()` |
| Library | `com.google.android.play:review` |
| Minimum API level | 21 (Android 5.0) |
| Throttling | Google enforces a quota — the prompt may not appear every time |
| Execution | Runs on a background thread to avoid blocking the UI |

**Required Gradle dependency:** The Google Play In-App Review API is not part of the Android SDK. You must add it to your app's `build.gradle`:

```groovy
dependencies {
    implementation 'com.google.android.play:review:2.0.2'
}
```

Without this dependency, `requestReview()` will resolve with an error explaining the missing library.

### Other platforms

On unsupported platforms (Linux, Windows, Web), `requestReview()` resolves immediately with an error. It will not throw — your app continues normally.

## Best practices

**Do ask at the right moment.** Prompt after a positive experience — completing a level, finishing a task, achieving a goal. Don't ask on first launch or during onboarding.

**Don't ask too often.** Even though iOS throttles automatically, Android does not have the same strict limits. Implement your own logic to track when you last asked:

```text
import { requestReview } from "perry-appstore-review";
import { preferencesGet, preferencesSet } from "perry/system";

async function maybeAskForReview() {
  const lastAsked = Number(preferencesGet("lastReviewAsk") || "0");
  const now = Date.now();
  const thirtyDays = 30 * 24 * 60 * 60 * 1000;

  if (now - lastAsked > thirtyDays) {
    preferencesSet("lastReviewAsk", String(now));
    await requestReview();
  }
}
```

**Don't condition app behavior on the review.** Neither iOS nor Android tells you whether the user left a review, gave a rating, or dismissed the prompt. The promise resolving does not mean a review was submitted.

**Don't use custom review dialogs before the native one.** Both Apple and Google discourage showing your own "Rate this app?" dialog before the native prompt. The native prompt is designed to be low-friction — adding a pre-prompt increases abandonment.

## Extension structure

The extension follows the standard [native extension](native-extensions.md) layout:

```
perry-appstore-review/
├── package.json              # Declares sb_appreview_request function
├── src/
│   └── index.ts              # Exports requestReview()
├── crate-ios/                # iOS/macOS: Swift → SKStoreReviewController
│   ├── Cargo.toml
│   ├── build.rs              # Compiles Swift to static library
│   ├── src/lib.rs            # Rust FFI bridge
│   └── swift/review_bridge.swift
├── crate-android/            # Android: JNI → Play In-App Review API
│   ├── Cargo.toml
│   └── src/lib.rs
└── crate-stub/               # Other platforms: resolves with error
    ├── Cargo.toml
    └── src/lib.rs
```

One native function is declared in `package.json`:

```json
{
  "perry": {
    "nativeLibrary": {
      "functions": [
        { "name": "sb_appreview_request", "params": [], "returns": "f64" }
      ]
    }
  }
}
```

The TypeScript layer wraps this into the public `requestReview()` function. The native layer creates a Perry promise, calls the platform API, and resolves the promise when done.

## Next Steps

- [Native Extensions](native-extensions.md) — How native extensions work, creating your own
- [iOS Platform](../platforms/ios.md) — iOS platform guide
- [Android Platform](../platforms/android.md) — Android platform guide
