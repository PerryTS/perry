// Repro for #763 / #764: reactive Text(`...${state.value}...`) inside a
// program that also two-way-binds a TextField to the same state.
import {
    App,
    Button,
    HStack,
    State,
    stateBindTextfield,
    Text,
    TextField,
    VStack,
} from 'perry/ui';

function main(): void {
    const text = State("");

    const field = TextField("Type something", (value) => {
        text.set(value);
    });

    stateBindTextfield(text, field);

    const setHello = Button("set hello world", () => {
        text.set("Hello, World!");
    });

    App({
        title: "Reactive Text Repro",
        width: 400,
        height: 300,
        body: VStack(12, [
            HStack(8, [field]),
            HStack(8, [setHello]),
            // The label should track `text` reactively. Empty at start;
            // should read "current state for text: Hello, World!" after the
            // button is pressed, or echo TextField input as you type.
            Text(`current state for text: ${text.value}`),
        ]),
    });
}

main();
