# In-process Servo web engine (experimental)

`perry-ui-macos` can optionally use [Servo](https://servo.org) as an alternative
web engine to the system WKWebView, selected at runtime via `PERRY_WEBVIEW=servo`.
Enabled by the `servo-webview` cargo feature (off by default — pulls the full
Servo stack incl. SpiderMonkey).

## Dependency conflicts (and how they're resolved)

Embedding Servo in-process means unifying its ~835-crate tree with perry's. Two
hard version conflicts had to be resolved:

1. **`libsqlite3-sys`** (`links = "sqlite3"` → one version per workspace).
   Servo pulls `rusqlite 0.37 → libsqlite3-sys 0.35`; perry pinned older.
   Fixed by upgrading perry's sqlite stack: `rusqlite 0.32→0.37` and
   `sqlx 0.8.6→0.9.0` (see the `chore/align-libsqlite3-for-servo` PR). Clean,
   stable upgrade — landed independently of Servo.

2. **`ml-kem`** (post-quantum KEM, used by both perry's WebCrypto and Servo's).
   Perry needs `ml-kem 0.3.2` + the `pkcs8` feature (stable `kem 0.3.0`); Servo's
   `servo-script` needs `ml-kem 0.2.x`, which pins a *pre-release* `kem
   =0.3.0-pre.0` and lacks `pkcs8`. Irreconcilable in one workspace.
   Fixed by **forking `servo-script`** and migrating its WebCrypto ML-KEM code
   from ml-kem 0.2 → 0.3.2 (so both sides share 0.3.2). The migration is captured
   in `servo-script-ml-kem-0.3.2.patch` (332 lines, 3 files) and applied via a
   local `[patch.crates-io]`.

   `ml-dsa` (the sibling ML-DSA/Dilithium crate Servo also uses) is **not** a
   conflict — perry doesn't depend on it.

## The fork

The migrated `servo-script` is hosted at
**https://github.com/PerryTS/servo-script-mlkem** — a vendored copy of crates.io
`servo-script 0.1.0` whose *only* change vs upstream is the ML-KEM migration
(captured in `servo-script-ml-kem-0.3.2.patch`). The workspace `Cargo.toml`
patches it in by git rev, so the branch resolves on any machine (incl. CI):

```toml
[patch.crates-io]
servo-script = { git = "https://github.com/PerryTS/servo-script-mlkem", rev = "<sha>" }
```

To regenerate the fork from scratch (e.g. to bump the servo version): vendor a
fresh `servo-script-0.1.0` from the registry, apply
`servo-script-ml-kem-0.3.2.patch`, and push to the fork repo, then update the
`rev` above.

## Selecting the Servo backend

- **At build time:** `perry compile app.ts --webview servo -o app` — links the
  Servo-enabled UI lib variant (`libperry_ui_macos_servo.a`) so the app defaults
  to Servo. Build that variant with
  `cargo build --release -p perry-ui-macos --features servo-webview` and place it
  alongside `libperry_ui_macos.a` (cargo emits the same filename regardless of
  features, so rename/scope it as the `_servo` variant). The compiler fails an
  explicit Servo build if that archive is missing.
- **At runtime:** the Servo-linked variant defaults to Servo.
  `PERRY_WEBVIEW=system` explicitly selects WKWebView; `PERRY_WEBVIEW=servo`
  explicitly selects Servo.

Electron-compatible `BrowserWindow` instances currently force the system
backend because Servo does not yet implement their document-start preload and
renderer IPC contracts. Ordinary `perry/ui` WebView widgets use the selection
above.

## Engine capabilities & limitations

The `ServoEngine` (`servo_webview.rs`) drives the WebView widget through the same
FFI surface as WKWebView:

- ✅ Navigation (`loadUrl` / `reload`), `evaluateJavaScript` → string callback,
  `onLoaded` on `LoadStatus::Complete`.
- ✅ Rendering — offscreen software render blitted into a layer-backed
  `NSImageView` at ~60 Hz.
- ✅ Interaction — a `PerryServoView` (flipped, first-responder `NSView`) forwards
  mouse down/up/drag, left/right buttons, and scroll wheel to Servo, and resizes
  the engine (rendering context + WebView) on layout via `setFrameSize:`.
- ⏳ Follow-ups: keyboard input (key-code → `KeyboardEvent` translation), hover
  (`mouseMoved` needs an `NSTrackingArea`), and HiDPI backing-scale (the software
  context renders at logical pixels today).

## ML-KEM migration: correctness note

The migration is **type-correct** (the full Servo stack compiles). Behavioral
equivalence was reasoned per call-site (`from_seed` ≡ `generate_deterministic`,
`ExpandedKeyEncoding::to_expanded_bytes` ≡ the old `EncodedSizeUser::as_bytes`,
no-arg `encapsulate()` OS entropy ≡ `OsRng`) but **not** verified against Servo's
WebCrypto ML-KEM test vectors. ML-KEM WebCrypto is a niche surface; page
rendering is unaffected. Verify with Servo's WPT WebCrypto suite before relying
on it.
