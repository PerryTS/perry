// demonstrates: minimal stateful UI — label + increment button
// docs: docs/src/ui/state.md
// platforms: macos, linux, windows
// targets: ios-simulator, visionos-simulator, tvos-simulator, watchos-simulator, android, web, wasm

import { App, VStack, Text, Button, State } from "perry/ui"

const count = State(0)

App({
    title: "Counter",
    width: 400,
    height: 300,
    body: VStack(16, [
        Text(`Count: ${count.value}`),
        Button("Increment", () => count.set(count.value + 1)),
    ]),
})
