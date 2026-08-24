// demonstrates: complete reactive todo app combining State, ForEach, and widget tree mutation
// docs: docs/src/ui/state.md
// platforms: macos, linux, windows, windows-winui
// targets: ios-simulator, tvos-simulator, watchos-simulator, web, wasm

import {
    App,
    Text,
    Button,
    TextField,
    VStack,
    HStack,
    State,
    ForEach,
    Spacer,
    Divider,
} from "perry/ui"

const todos = State<string[]>([])
const count = State(0)
const input = State("")

App({
    title: "Todo App",
    width: 480,
    height: 600,
    body: VStack(16, [
        Text("My Todos"),

        HStack(8, [
            TextField("What needs to be done?", (value: string) => input.set(value)),
            Button("Add", () => {
                const text = input.value
                if (text.length > 0) {
                    todos.set([...todos.value, text])
                    count.set(count.value + 1)
                    input.set("")
                }
            }),
        ]),

        Divider(),

        ForEach(count, (i: number) =>
            HStack(8, [
                Text(todos.value[i]),
                Spacer(),
                Button("Delete", () => {
                    todos.set(todos.value.filter((_, idx) => idx !== i))
                    count.set(count.value - 1)
                }),
            ]),
        ),

        Spacer(),
        Text(`${count.value} items`),
    ]),
})
