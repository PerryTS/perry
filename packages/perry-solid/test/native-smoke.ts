import { App, VStack, widgetAddChild, type Widget } from "perry/ui";
import { createSignal } from "solid-js";
import { h, render, For, type NativeNode } from "../src/index.ts";

const [count, setCount] = createSignal(0);
const [items, setItems] = createSignal(["Alpha", "Beta", "Gamma"]);
const body = VStack([]);
let counter: NativeNode;
let increment: NativeNode;
let rotate: NativeNode;
let list: NativeNode;
const dispose = render(() => {
  counter = h("Text", { fontSize: 24 }, () => `Count: ${count()}`);
  increment = h("Button", { onPress: () => setCount(n => n + 1) }, "Increment");
  rotate = h("Button", { onPress: () => setItems(rows => [rows[2], rows[0], rows[1]]) }, "Rotate");
  list = h("VStack", null, For({
    get each() { return items(); },
    children: item => h("Text", null, item),
  }));
  return h("VStack", { padding: 16 }, counter, increment, rotate, h("VStack", null, () => `Raw: ${count()}`), list);
}, body);
const stop = h("Button", { onPress: () => { dispose(); setCount(99); } }, "Dispose");
// Keep the disposal control outside the mounted Solid root for the smoke test.
widgetAddChild(body, stop.handle as Widget);
const exit = h("Button", { onPress: () => process.exit(0) }, "Exit");
widgetAddChild(body, exit.handle as Widget);
App({ title: "Solid native smoke", width: 420, height: 360, body });
