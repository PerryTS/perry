// demonstrates: minimal Wear OS App() lifecycle snippet from the Wear OS docs page
// docs: docs/src/platforms/wearos.md
// platforms: macos, linux, windows

// On Wear OS this lowers through the same Android View backend (perry-ui-android,
// JNI → TextView/LinearLayout/Button/...) used for phone apps — Wear OS *is*
// Android on a watch. `perry run wearos` builds the same `.so`, then packages it
// with the watch form-factor overlay (uses-feature android.hardware.type.watch,
// standalone, androidx.wear). On macOS / Linux / Windows the same TypeScript
// lowers through the host's native UI library.

// ANCHOR: wearos-app
import { App, Text, VStack, Button, State } from "perry/ui"

const count = State(0)

App({
    title: "My Watch App",
    width: 200,
    height: 200,
    body: VStack(8, [
        Text("Hello, Wear OS!"),
        Text(`Taps: ${count.value}`),
        Button("Tap me", () => count.set(count.value + 1)),
    ]),
})
// ANCHOR_END: wearos-app
