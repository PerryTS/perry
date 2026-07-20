// Perry Windows UI demo — core widgets on the Win32 backend.
//
// Exercises Text (reactive via setText), Button, TextField, Slider, and a
// setInterval timer, all serviced by the Win32 message pump during App.run().
//
// Build & run (from the repo root, or with an installed perry):
//   perry examples/windows_ui_demo.ts -o windows_ui_demo
//   .\windows_ui_demo.exe
//
// Or straight from a compiler checkout:
//   cargo run --release -- examples/windows_ui_demo.ts -o windows_ui_demo

import { App, VStack, HStack, Text, Button, TextField, Slider, setText } from "perry/ui"

let clicks = 0
let ticks = 0

// setInterval is serviced by the Win32 message pump during App.run().
setInterval(() => {
    ticks++
    setText("ticks", `ticks: ${ticks}`)
}, 500)

App({
    title: "Perry Windows Demo",
    width: 480,
    height: 340,
    body: VStack(12, [
        Text("Perry Windows UI demo"),
        Text("clicks: 0", "clicks"),
        Text("ticks: 0", "ticks"),
        Text("typed: (nothing)", "typed"),
        Text("slider: 0", "slider"),
        HStack(8, [
            Button("Click me", () => {
                clicks++
                setText("clicks", `clicks: ${clicks}`)
            }),
        ]),
        TextField("type here", (v: string) => {
            setText("typed", `typed: ${v}`)
        }),
        Slider(0, 100, (v: number) => {
            setText("slider", `slider: ${v}`)
        }),
    ]),
})
