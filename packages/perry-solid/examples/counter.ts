import { App, VStack } from "perry/ui";
import { createSignal } from "solid-js";
import { h, render, For } from "../src/index.ts";

function Counter() {
  const [count, setCount] = createSignal(0);
  const [items, setItems] = createSignal(["Alpha", "Beta", "Gamma"]);
  return h("VStack", { padding: 16 },
    h("Text", { fontSize: 24 }, () => `Count: ${count()}`),
    h("HStack", null,
      h("Button", { onPress: () => setCount(n => n + 1) }, "Increment"),
      h("Button", { onPress: () => setCount(0) }, "Reset"),
      h("Button", { onPress: () => setItems(rows => [rows[2], rows[0], rows[1]]) }, "Rotate"),
    ),
    h("VStack", null, For({
      get each() { return items(); },
      children: item => h("Text", null, item),
    })),
  );
}

const body = VStack([]);
render(Counter, body);
App({ title: "Solid + Perry", width: 420, height: 300, body });
