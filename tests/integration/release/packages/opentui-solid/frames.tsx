import { createSignal, For } from "solid-js";
import { testRender, effect } from "@opentui/solid";

const [count, setCount] = createSignal(0);
const [props, setProps] = createSignal({ title: "first", padding: 0 });
const [rows, setRows] = createSignal(["A", "B"]);
let label: any;
let seen = 0;
function watch(node: any, value: () => number) { effect(() => { seen = value(); }); }
function Label(props: any) { return <text>{props.value}</text>; }
const view = await testRender(() => <box width={32} height={8} {...props()} padding={0}>
  <text ref={label} use:watch={count()} fg={count() > 0 ? "green" : "white"}>Count {count()}</text>
  <Label value={"component " + count()} />
  <For each={rows()}>{row => <text>{row}</text>}</For>
</box>, { width: 32, height: 8 });
await view.renderOnce();
const frames = [view.captureCharFrame()];
setCount(1);
setRows(["B", "A"]);
setProps({ title: "second", padding: 2 });
await view.renderOnce();
frames.push(view.captureCharFrame());
if (!label || seen !== 1) throw new Error("refs/directive did not update");
view.renderer.destroy();
console.log(JSON.stringify(frames));
