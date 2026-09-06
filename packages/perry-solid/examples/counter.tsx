import { App, VStack } from "perry/ui";
import { createSignal } from "solid-js";
import { For, render } from "perry-solid";

function Counter() {
  const [count, setCount] = createSignal(0);
  const [items, setItems] = createSignal(["Alpha", "Beta", "Gamma"]);
  return <vstack padding={16}>
    <text fontSize={24}>Count: {count()}</text>
    <hstack>
      <button onPress={() => setCount(value => value + 1)}>Increment</button>
      <button onPress={() => setCount(0)}>Reset</button>
      <button onPress={() => setItems(rows => [rows[2], rows[0], rows[1]])}>Rotate</button>
    </hstack>
    <vstack><For each={items()}>{item => <text>{item}</text>}</For></vstack>
  </vstack>;
}

const body = VStack([]);
render(() => <Counter />, body);
App({ title: "Solid JSX + Perry", width: 420, height: 300, body });
