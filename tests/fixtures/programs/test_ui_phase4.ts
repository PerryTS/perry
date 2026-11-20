import { App, VStack, HStack, Text, Button, State, Slider, Toggle, ForEach, Spacer } from "perry/ui"

const a = State(3)
const b = State(5)
const dark = State(0)     // 0 = off, 1 = on
const count = State(3)

App({
    title: "Phase 4 Demo",
    width: 500, height: 500,
    body: VStack(12, [
        // Feature 1: Multi-state text
        Text(`${a.value} + ${b.value} = ${a.value + b.value}`),

        // Feature 2: Two-way binding (slider reflects state)
        Slider(0, 10, a.value, (v: number) => a.set(v)),
        Button("a+1", () => a.set(a.value + 1)),

        Slider(0, 10, b.value, (v: number) => b.set(v)),
        Button("b+1", () => b.set(b.value + 1)),

        // Feature 3: Conditional rendering
        dark.value ? Text("Dark Mode ON") : Text("Dark Mode OFF"),
        Toggle("Dark mode", (on: boolean) => dark.set(on ? 1 : 0)),

        // Feature 4: Dynamic list
        ForEach(count, (i: number) => Text(`Item ${i}`)),
        HStack(8, [
            Button("+", () => count.set(count.value + 1)),
            Button("-", () => count.set(count.value - 1)),
        ]),

        Spacer(),
    ])
})
