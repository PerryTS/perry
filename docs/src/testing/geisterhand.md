# Geisterhand — In-Process UI Testing

Geisterhand (German for "ghost hand") embeds a lightweight HTTP server inside your Perry app that lets you interact with every widget programmatically. Click buttons, type into text fields, drag sliders, toggle switches, capture screenshots, and run chaos-mode random fuzzing — all via simple HTTP calls.

It works on **all 5 native platforms** (macOS, iOS, Android, Linux/GTK4, Windows) with zero external dependencies. The server starts automatically when you compile with `--enable-geisterhand`.

---

## Quick Start

```bash
# 1. Compile with geisterhand enabled (libs auto-build on first use)
perry app.ts -o app --enable-geisterhand

# 2. Run the app
./app
# [geisterhand] listening on http://127.0.0.1:7676

# 3. In another terminal — interact with the app
curl http://127.0.0.1:7676/widgets            # List all widgets
curl -X POST http://127.0.0.1:7676/click/3     # Click button with handle 3
curl http://127.0.0.1:7676/screenshot -o s.png # Capture window screenshot
```

### Custom Port

The default port is **7676**. Use `--geisterhand-port` to change it (this implies `--enable-geisterhand`, so you don't need both flags):

```bash
perry app.ts -o app --geisterhand-port 9090
# or with perry run:
perry run --geisterhand-port 9090
```

### With `perry run`

```bash
perry run --enable-geisterhand
perry run macos --geisterhand-port 8080
perry run ios --enable-geisterhand
```

---

## API Reference

All endpoints return JSON unless noted otherwise. All responses include `Access-Control-Allow-Origin: *` for browser-based tools. OPTIONS requests are supported for CORS preflight.

### Health Check

```
GET /health
→ {"status":"ok"}
```

Use this to wait for the app to be ready before running tests.

### List Widgets

```
GET /widgets
```

Returns a JSON array of all registered widgets:

```json
[
  {"handle": 3, "widget_type": 0, "callback_kind": 0, "label": "Click Me", "shortcut": ""},
  {"handle": 4, "widget_type": 1, "callback_kind": 1, "label": "Type here...", "shortcut": ""},
  {"handle": 5, "widget_type": 2, "callback_kind": 1, "label": "", "shortcut": ""},
  {"handle": 6, "widget_type": 3, "callback_kind": 1, "label": "Enable", "shortcut": ""},
  {"handle": 7, "widget_type": 5, "callback_kind": 0, "label": "Save", "shortcut": "s"},
  {"handle": 8, "widget_type": 8, "callback_kind": 0, "label": "", "shortcut": ""}
]
```

Supports query parameter filters:
- `GET /widgets?label=Save` — filter by label substring (case-insensitive)
- `GET /widgets?type=button` — filter by widget type name or code
- `GET /widgets?label=Save&type=5` — combine filters

#### Widget Types

| Code | Type | Description |
|------|------|-------------|
| 0 | Button | Push button with onClick |
| 1 | TextField | Text input field |
| 2 | Slider | Numeric slider |
| 3 | Toggle | On/off switch |
| 4 | Picker | Dropdown selector |
| 5 | Menu | Menu item |
| 6 | Shortcut | Keyboard shortcut |
| 7 | Table | Data table |
| 8 | ScrollView | Scrollable container |

#### Callback Kinds

| Code | Kind | Description |
|------|------|-------------|
| 0 | onClick | Triggered on click/tap |
| 1 | onChange | Triggered on value change |
| 2 | onSubmit | Triggered on submit (e.g., pressing Enter) |
| 3 | onHover | Triggered on mouse hover |
| 4 | onDoubleClick | Triggered on double-click |
| 5 | onFocus | Triggered on focus |

A single widget may appear multiple times in the list with different callback kinds. For example, a button with both `onClick` and `onHover` handlers produces two entries (same handle, different `callback_kind`).

### Click a Widget

```
POST /click/:handle
→ {"ok":true}
```

Fires the widget's `onClick` callback. Works with buttons, menu items, shortcuts, and table rows.

```bash
curl -X POST http://127.0.0.1:7676/click/3
```

### Type into a TextField

```
POST /type/:handle
Content-Type: application/json

{"text": "hello world"}
```

Sets the text field's content and fires its `onChange` callback with the new text as a NaN-boxed string.

```bash
curl -X POST http://127.0.0.1:7676/type/4 \
  -H 'Content-Type: application/json' \
  -d '{"text":"hello world"}'
```

### Move a Slider

```
POST /slide/:handle
Content-Type: application/json

{"value": 0.75}
```

Sets the slider position and fires `onChange` with the numeric value.

```bash
curl -X POST http://127.0.0.1:7676/slide/5 \
  -H 'Content-Type: application/json' \
  -d '{"value":0.75}'
```

### Toggle a Switch

```
POST /toggle/:handle
→ {"ok":true}
```

Fires the toggle's `onChange` callback with a boolean value.

```bash
curl -X POST http://127.0.0.1:7676/toggle/6
```

### Set State Directly

```
POST /state/:handle
Content-Type: application/json

{"value": 42}
```

Directly sets a `State` cell's value, bypassing widget callbacks. This triggers any reactive bindings attached to the state (bound text labels, visibility, forEach loops, etc.).

```bash
curl -X POST http://127.0.0.1:7676/state/2 \
  -H 'Content-Type: application/json' \
  -d '{"value":42}'
```

### Hover

```
POST /hover/:handle
→ {"ok":true}
```

Fires the widget's `onHover` callback. Useful for testing hover-dependent UI (tooltips, color changes, etc.).

### Double-Click

```
POST /doubleclick/:handle
→ {"ok":true}
```

Fires the widget's `onDoubleClick` callback.

### Trigger Keyboard Shortcut

```
POST /key
Content-Type: application/json

{"shortcut": "s"}
```

Finds a registered menu item whose shortcut matches and fires its callback. Shortcut strings are case-insensitive and match the key string passed to `menuAddItem` (e.g., `"s"` for Cmd+S, `"S"` for Cmd+Shift+S, `"n"` for Cmd+N).

```bash
curl -X POST http://127.0.0.1:7676/key \
  -H 'Content-Type: application/json' \
  -d '{"shortcut":"s"}'
```

Returns `{"ok":true}` if a matching shortcut was found, or 404 if no match.

### Scroll a ScrollView

```
POST /scroll/:handle
Content-Type: application/json

{"x": 0, "y": 100}
```

Sets the scroll offset of a ScrollView widget. Both `x` and `y` are in points.

```bash
curl -X POST http://127.0.0.1:7676/scroll/8 \
  -H 'Content-Type: application/json' \
  -d '{"x":0,"y":200}'
```

### Capture Screenshot

```
GET /screenshot
→ (binary PNG image, Content-Type: image/png)
```

Captures the app window as a PNG image. The response is raw binary data, not JSON.

```bash
curl http://127.0.0.1:7676/screenshot -o screenshot.png
```

Screenshot capture is synchronous from the caller's perspective — the HTTP request blocks until the main thread completes the capture (timeout: 5 seconds).

**Platform-specific capture methods:**

| Platform | Method | Notes |
|----------|--------|-------|
| macOS | `CGWindowListCreateImage` | Retina resolution, reads from window ID |
| iOS | `UIGraphicsImageRenderer` | Draws view hierarchy into image context |
| Android | JNI `View.draw()` on Canvas | Creates Bitmap, compresses to PNG |
| Linux (GTK4) | `WidgetPaintable` + `GskRenderer` | Renders to texture, saves as PNG bytes |
| Windows | `PrintWindow` + `GetDIBits` | Inline PNG encoder (stored zlib blocks) |

### Chaos Mode

Chaos mode randomly interacts with widgets at a configurable interval — useful for stress testing, finding edge cases, and crash hunting.

#### Start

```
POST /chaos/start
Content-Type: application/json

{"interval_ms": 200}
```

```bash
# Fire random inputs every 200ms
curl -X POST http://127.0.0.1:7676/chaos/start \
  -H 'Content-Type: application/json' \
  -d '{"interval_ms":200}'
```

If `interval_ms` is omitted, a default interval is used. The chaos thread randomly selects a registered widget and fires an appropriate input based on widget type:

| Widget Type | Random Input |
|-------------|-------------|
| Button | Fires onClick (no args) |
| TextField | Random alphanumeric string, 5-20 characters |
| Slider | Random float between 0.0 and 1.0 |
| Toggle | Random true/false |
| Picker | Random index 0-9 |
| Menu | Fires onClick (no args) |
| Shortcut | Fires onClick (no args) |
| Table | Fires onClick (no args) |

#### Status

```
GET /chaos/status
→ {"running":true,"events_fired":247,"uptime_secs":12}
```

Returns whether chaos mode is active, how many random events have been fired, and uptime in seconds.

#### Stop

```
POST /chaos/stop
→ {"ok":true,"chaos":"stopped"}
```

### Error Responses

All endpoints return errors as JSON with an appropriate HTTP status code:

```json
{"error": "widget handle 99 not found"}
```

Common errors:
- `404` — widget handle not found
- `400` — malformed JSON body or missing required field
- `405` — unsupported HTTP method

---

## Platform Setup

### macOS

No extra setup needed. The server binds to `0.0.0.0:7676` and is accessible on `localhost`.

```bash
perry app.ts -o app --enable-geisterhand
./app
curl http://127.0.0.1:7676/widgets
```

### iOS Simulator

The iOS Simulator shares the host's network stack — access the server directly on `localhost`:

```bash
perry app.ts -o app --target ios-simulator --enable-geisterhand
xcrun simctl install booted app.app
xcrun simctl launch booted com.perry.app
curl http://127.0.0.1:7676/widgets
```

### iOS Device

For physical iOS devices, you need a network route to the device (same Wi-Fi network) or use `iproxy` from `libimobiledevice`:

```bash
perry app.ts -o app --target ios --enable-geisterhand
# Install and launch via Xcode/devicectl
# Then connect via the device's IP:
curl http://192.168.1.42:7676/widgets
```

### Android (Emulator or Device)

Use `adb forward` to bridge the port. Ensure `INTERNET` permission is in your manifest (or add it to `perry.toml`):

```toml
[android]
permissions = ["INTERNET"]
```

```bash
perry app.ts -o app --target android --enable-geisterhand
# Package into APK and install
adb forward tcp:7676 tcp:7676
curl http://127.0.0.1:7676/widgets
```

### Linux (GTK4)

Install GTK4 development libraries first:

```bash
# Ubuntu/Debian
sudo apt install libgtk-4-dev libcairo2-dev

perry app.ts -o app --target linux --enable-geisterhand
./app
curl http://127.0.0.1:7676/widgets
```

### Windows

```bash
perry app.ts -o app --target windows --enable-geisterhand
./app.exe
curl http://127.0.0.1:7676/widgets
```

---

## Test Automation

Geisterhand turns your Perry app into a testable HTTP service. Here are practical patterns for automated testing.

### Shell Script Tests

A simple end-to-end test using bash:

```bash
#!/bin/bash
set -e

# Build with geisterhand
perry app.ts -o testapp --enable-geisterhand

# Start the app in background
./testapp &
APP_PID=$!
trap "kill $APP_PID 2>/dev/null" EXIT

# Wait for the app to be ready
for i in $(seq 1 30); do
  curl -sf http://127.0.0.1:7676/health && break
  sleep 0.1
done

# Get widgets
WIDGETS=$(curl -sf http://127.0.0.1:7676/widgets)
echo "Registered widgets: $WIDGETS"

# Find the button labeled "Submit"
SUBMIT_HANDLE=$(echo "$WIDGETS" | jq -r '.[] | select(.label == "Submit") | .handle')

# Click it
curl -sf -X POST "http://127.0.0.1:7676/click/$SUBMIT_HANDLE"

# Take a screenshot after interaction
curl -sf http://127.0.0.1:7676/screenshot -o after-click.png

echo "Test passed"
```

### Python Test Example

```python
import subprocess, time, requests, json

# Start the app
proc = subprocess.Popen(["./testapp"])
time.sleep(1)  # Wait for startup

try:
    # List widgets
    widgets = requests.get("http://127.0.0.1:7676/widgets").json()

    # Find widgets by label
    buttons = [w for w in widgets if w["widget_type"] == 0]
    fields = [w for w in widgets if w["widget_type"] == 1]

    # Type into the first text field
    if fields:
        requests.post(
            f"http://127.0.0.1:7676/type/{fields[0]['handle']}",
            json={"text": "test@example.com"}
        )

    # Click the first button
    if buttons:
        requests.post(f"http://127.0.0.1:7676/click/{buttons[0]['handle']}")

    # Capture screenshot for visual regression
    png = requests.get("http://127.0.0.1:7676/screenshot").content
    with open("test-result.png", "wb") as f:
        f.write(png)

    # Assert the app is still healthy
    assert requests.get("http://127.0.0.1:7676/health").json()["status"] == "ok"
    print("All tests passed")
finally:
    proc.terminate()
```

### Stress Testing with Chaos Mode

Run chaos mode against your app to find crashes, freezes, or unexpected state:

```bash
# Build and launch
perry app.ts -o app --enable-geisterhand
./app &

# Wait for startup
sleep 1

# Start aggressive chaos (every 50ms)
curl -X POST http://127.0.0.1:7676/chaos/start \
  -H 'Content-Type: application/json' \
  -d '{"interval_ms":50}'

# Let it run for 30 seconds
sleep 30

# Check stats
curl -sf http://127.0.0.1:7676/chaos/status
# {"running":true,"events_fired":600,"uptime_secs":30}

# Take a screenshot to see final state
curl http://127.0.0.1:7676/screenshot -o chaos-result.png

# Stop chaos
curl -X POST http://127.0.0.1:7676/chaos/stop

# Check the app is still alive
curl -sf http://127.0.0.1:7676/health
```

### Visual Regression Testing

Capture screenshots at key interaction points and compare against baselines:

```bash
# Initial state
curl http://127.0.0.1:7676/screenshot -o baseline.png

# Interact
curl -X POST http://127.0.0.1:7676/click/3
curl -X POST http://127.0.0.1:7676/type/4 -d '{"text":"Hello"}'

# Capture after interaction
curl http://127.0.0.1:7676/screenshot -o current.png

# Compare (using ImageMagick)
compare baseline.png current.png diff.png
```

### CI Pipeline Integration

```yaml
# GitHub Actions example
jobs:
  ui-test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4

      - name: Build with geisterhand
        run: perry app.ts -o testapp --enable-geisterhand

      - name: Run UI tests
        run: |
          ./testapp &
          sleep 2
          # Run your test script
          ./tests/ui-test.sh
          kill %1

      - name: Upload screenshots
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: screenshots
          path: "*.png"
```

---

## Example App

A complete Perry UI app demonstrating every widget type Geisterhand can
interact with — verified by CI:

```typescript
{{#include ../../examples/ui/testing/geisterhand_demo.ts}}
```

After compiling with `--enable-geisterhand` and running:

```bash
# See all interactive widgets
curl -s http://127.0.0.1:7676/widgets | jq .
# [
#   {"handle":3,"widget_type":0,"callback_kind":0,"label":"Increment"},
#   {"handle":4,"widget_type":0,"callback_kind":0,"label":"Reset"},
#   {"handle":5,"widget_type":1,"callback_kind":1,"label":"Enter your name"},
#   {"handle":6,"widget_type":2,"callback_kind":1,"label":""},
#   {"handle":7,"widget_type":3,"callback_kind":1,"label":"Dark Mode"}
# ]

# Click Increment 3 times
for i in 1 2 3; do curl -sX POST http://127.0.0.1:7676/click/3; done
# Counter label now shows "Count: 3"

# Type a name
curl -sX POST http://127.0.0.1:7676/type/5 -d '{"text":"Perry"}'

# Set slider to 80%
curl -sX POST http://127.0.0.1:7676/slide/6 -d '{"value":0.8}'

# Toggle dark mode on
curl -sX POST http://127.0.0.1:7676/toggle/7

# Screenshot
curl -s http://127.0.0.1:7676/screenshot -o demo.png
```

---

## Architecture

Geisterhand operates as three cooperating components connected by thread-safe queues:

```
                    ┌──────────────────────────┐
                    │      HTTP Server         │
                    │   (background thread)    │
                    │   tiny-http on :7676     │
                    │                          │
                    │  GET /widgets            │
                    │  POST /click/:h          │
                    │  POST /type/:h           │
                    │  ...                     │
                    └────────┬─────────────────┘
                             │
                    queue actions via
                    Mutex<Vec<PendingAction>>
                             │
                             ▼
┌────────────────────────────────────────────────┐
│                 Main Thread                     │
│                                                 │
│  perry_geisterhand_pump() ← called every 8ms   │
│  by platform timer (NSTimer / glib / WM_TIMER)  │
│                                                 │
│  Drains PendingAction queue:                    │
│  • InvokeCallback → js_closure_call0/1          │
│  • SetState → perry_ui_state_set                │
│  • CaptureScreenshot → perry_ui_screenshot_*    │
└────────────────────────────────────────────────┘
                             │
                    widget callbacks registered
                    at creation time via
                    perry_geisterhand_register()
                             │
                             ▼
┌────────────────────────────────────────────────┐
│            Global Widget Registry              │
│         Mutex<Vec<RegisteredWidget>>           │
│                                                │
│  { handle, widget_type, callback_kind,         │
│    closure_f64, label }                        │
└────────────────────────────────────────────────┘
```

### Lifecycle

1. **Startup**: When `--enable-geisterhand` is used, the compiled binary calls `perry_geisterhand_start(port)` during initialization. This spawns a background thread running a `tiny-http` server.

2. **Widget Registration**: As UI widgets are created (Button, TextField, Slider, etc.), each one calls `perry_geisterhand_register(handle, widget_type, callback_kind, closure_f64, label)` to register its callback in the global registry. This is gated behind `#[cfg(feature = "geisterhand")]` so normal builds have zero overhead.

3. **HTTP Requests**: When a request arrives (e.g., `POST /click/3`), the server looks up handle 3 in the registry, finds the associated closure, and pushes a `PendingAction::InvokeCallback` onto the pending actions queue.

4. **Main-Thread Dispatch**: The platform's timer (NSTimer on macOS, glib timeout on GTK4, WM_TIMER on Windows, etc.) calls `perry_geisterhand_pump()` every ~8ms. This drains the pending actions queue and executes callbacks on the main thread, which is required for UI safety.

5. **Screenshot Capture**: Screenshots use `Condvar` synchronization — the HTTP thread queues a `CaptureScreenshot` action, then blocks waiting on a condition variable. The main thread's pump executes the platform-specific capture, stores the PNG data, and signals the condvar. Timeout: 5 seconds.

### Thread Safety

- **Widget Registry**: Protected by `Mutex`. Read by the HTTP server (to list widgets and look up handles), written by the main thread (during widget creation).
- **Pending Actions Queue**: Protected by `Mutex`. Written by HTTP server thread, drained by main thread in `pump()`.
- **Screenshot Result**: Protected by `Mutex` + `Condvar`. HTTP thread waits, main thread signals.
- **Chaos Mode State**: Uses `AtomicBool` (running flag) and `AtomicU64` (event counter) for lock-free status checks.

### NaN-Boxing Bridge

When geisterhand needs to pass values to widget callbacks, it must create properly NaN-boxed values:

- **Strings** (for TextField): Calls `js_string_from_bytes(ptr, len)` to allocate a runtime string, then `js_nanbox_string(ptr)` to wrap it with STRING_TAG (0x7FFF).
- **Numbers** (for Slider): Passes the raw `f64` value directly (numbers are their own NaN-boxed representation).
- **Booleans** (for Toggle/chaos): Uses `TAG_TRUE` (0x7FFC000000000004) or `TAG_FALSE` (0x7FFC000000000003).

---

## Build Details

### Auto-Build

When you pass `--enable-geisterhand` (or `--geisterhand-port`), Perry asks Cargo to build the complete library set together before linking. Cargo reuses fresh artifacts; existing archives alone are not enough to prove that their bundled Rust runtimes are compatible. This requires a Perry source checkout, located automatically or through `PERRY_WORKSPACE_ROOT`:

```
CARGO_TARGET_DIR=target/geisterhand cargo build --release \
  -p perry-runtime-static --features perry-runtime/geisterhand \
  -p perry-stdlib-static \
  -p perry-ui-{platform} --features perry-ui-{platform}/geisterhand \
  -p perry-ui-geisterhand
```

Platform crate selection is automatic based on `--target`:

| Target | UI Crate |
|--------|----------|
| (default/macOS) | `perry-ui-macos` |
| `ios` / `ios-simulator` | `perry-ui-ios` |
| `android` | `perry-ui-android` |
| `linux` | `perry-ui-gtk4` |
| `windows` | `perry-ui-windows` |

### Separate Target Directory

Geisterhand libraries are built into `target/geisterhand/` (via `CARGO_TARGET_DIR`) to avoid interfering with normal builds. This means your first geisterhand build takes a moment, but subsequent builds reuse Cargo’s cache. Perry links the exact archive paths reported by that invocation, including when Cargo uses a target-triple subdirectory. Build the full set together when building manually as well; separate runtime and stdlib builds can embed incompatible runtime instances and crash during module initialization (#10019).

### Feature Flags

All geisterhand code is behind `#[cfg(feature = "geisterhand")]` feature gates:

- **`perry-runtime/geisterhand`**: Compiles the `geisterhand_registry` module — widget registry, action queue, pump function, screenshot coordination.
- **`perry-ui-{platform}/geisterhand`**: Adds `perry_geisterhand_register()` calls to widget constructors and `perry_geisterhand_pump()` to the platform timer.

When the feature is not enabled, no geisterhand code is compiled — zero binary size overhead and zero runtime cost.

### Linking

The compiled binary links three additional static libraries:
1. `libperry_runtime.a` (geisterhand-featured build, replaces the normal runtime)
2. `libperry_ui_{platform}.a` (geisterhand-featured build, replaces the normal UI lib)
3. `libperry_ui_geisterhand.a` (HTTP server + chaos mode)

### Manual Build

If auto-build fails or you want to cross-compile manually:

```bash
# Build geisterhand libs for macOS
CARGO_TARGET_DIR=target/geisterhand cargo build --release \
  -p perry-runtime --features perry-runtime/geisterhand \
  -p perry-ui-macos --features perry-ui-macos/geisterhand \
  -p perry-ui-geisterhand

# Build for iOS (cross-compile)
CARGO_TARGET_DIR=target/geisterhand cargo build --release \
  --target aarch64-apple-ios \
  -p perry-runtime --features perry-runtime/geisterhand \
  -p perry-ui-ios --features perry-ui-ios/geisterhand \
  -p perry-ui-geisterhand
```

---

## Security

Geisterhand binds to `0.0.0.0` on the configured port (default 7676). This means it is **accessible from the local network** — any device on the same network can interact with your app, capture screenshots, or trigger chaos mode.

**Do not ship geisterhand-enabled binaries to production or to end users.**

Geisterhand is a development and testing tool only. The feature-gate system ensures it cannot accidentally be included in normal builds — you must explicitly pass `--enable-geisterhand` or `--geisterhand-port`.

---

## Troubleshooting

### "Connection refused" on port 7676

- Ensure you compiled with `--enable-geisterhand` or `--geisterhand-port`
- Check that the app has fully started (look for `[geisterhand] listening on...` in stderr)
- Verify the port isn't in use by another process: `lsof -i :7676`

### Widget handles not found

- Handles are assigned at widget creation time. If you query `/widgets` before the UI is fully constructed, some widgets may not be registered yet.
- Wait for `GET /health` to return `{"status":"ok"}` before interacting.

### Screenshot returns empty data

- Screenshot capture has a 5-second timeout. If the main thread is blocked (e.g., by a long-running synchronous operation), the screenshot will time out and return empty data.
- On macOS, ensure the app has a visible window (minimized windows may not capture correctly).

### Auto-build fails

- Ensure you have a working Rust toolchain (`rustup show`)
- For cross-compilation targets, install the appropriate target: `rustup target add aarch64-apple-ios`
- Check that the Perry source tree is accessible (auto-build searches upward from the `perry` executable for the workspace root)

### Chaos mode crashes the app

That's the point — chaos mode found a bug. Check the app's stderr output for panic messages or stack traces. Common causes:
- Callback handlers that assume valid state but receive unexpected values
- Missing null checks on state values
- Race conditions in state updates
