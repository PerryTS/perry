import { App, VStack, Button, widgetAddChild } from "perry/ui";
import { createSignal } from "solid-js";
import { render, For } from "perry-solid";

const [count, setCount] = createSignal(0);
const [items, setItems] = createSignal(["Alpha", "Beta", "Gamma"]);
const body = VStack([]);
const dispose = render(() => <vstack padding={16}>
  <text fontSize={24}>Count: {count()}</text>
  <button onPress={() => setCount(value => value + 1)}>Increment</button>
  <button onPress={() => setItems(rows => [rows[2], rows[0], rows[1]])}>Rotate</button>
  <vstack>{"Raw: " + count()}</vstack>
  <vstack><For each={items()}>{item => <text>{item}</text>}</For></vstack>
</vstack>, body);
widgetAddChild(body, Button("Dispose", () => { dispose(); setCount(99); }));
widgetAddChild(body, Button("Exit", () => process.exit(0)));
App({ title: "Solid native smoke", width: 420, height: 360, body });
