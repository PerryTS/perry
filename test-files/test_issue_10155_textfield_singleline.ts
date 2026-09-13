// Smoke test for issue #10155: a fixed-width editable TextField on macOS must
// stay one line and scroll horizontally, not wrap and grow tall.
// Run the built app and confirm the long value shows on one line inside the
// 220px-wide field, matching a normal single-line NSTextField.
import { App, VStack, Text, TextField, State, stateBindTextfield, widgetSetWidth } from "perry/ui"

const text = State("assignee = currentUser() AND statusCategory != Done AND project = SU")
const field = TextField("query...", (value: string) => text.set(value))
stateBindTextfield(text, field)
widgetSetWidth(field, 220)

App({
    title: "issue 10155 single-line TextField",
    width: 300,
    height: 160,
    body: VStack(12, [
        Text("Field below must stay one line and scroll, not wrap:"),
        field,
    ]),
})
