// #4644: exercise the installed client runtime, including the shared owner
// and listener state used by solid-js/store and solid-js/universal.
import {
  batch, createComponent, createMemo, createRenderEffect, createRoot,
  createSignal, For, onCleanup, untrack,
} from "solid-js";
import { createStore, produce, reconcile, unwrap } from "solid-js/store";
import { createRenderer } from "solid-js/universal";

const state = createRoot(dispose => {
  const [count, setCount] = createSignal(1);
  const doubled = createMemo(() => count() * 2);
  const values: number[] = [];
  createRenderEffect(() => { values.push(doubled()); });
  onCleanup(() => console.log("signal cleanup"));
  return { count, setCount, doubled, values, dispose };
});
console.log("initial", state.count(), state.doubled(), state.values.join(","));
state.setCount(2);
console.log("update", state.count(), state.doubled(), state.values.join(","));
batch(() => { state.setCount(3); state.setCount(4); });
state.setCount(4); // Equal writes must not notify.
console.log("batch", state.count(), state.doubled(), state.values.join(","));
console.log("untrack", untrack(state.count));
state.dispose();
state.setCount(5);
console.log("disposed", state.values.join(","));

const branch = createRoot(dispose => {
  const [left, setLeft] = createSignal(true);
  const [a, setA] = createSignal(1);
  const [b, setB] = createSignal(10);
  const seen: number[] = [];
  let cleanups = 0;
  createRenderEffect(() => {
    seen.push(left() ? a() : b());
    onCleanup(() => { cleanups++; });
  });
  return { setLeft, setA, setB, seen, cleanups: () => cleanups, dispose };
});
branch.setB(11); // The inactive dependency is not subscribed.
branch.setA(2);
branch.setLeft(false);
branch.setA(3); // The old dependency must have been removed.
branch.setB(12);
console.log("branches", branch.seen.join(","), branch.cleanups());
branch.dispose();
console.log("branch cleanup", branch.cleanups());

const store = createRoot(dispose => {
  const [value, setValue] = createStore({
    user: { name: "Ada", score: 1 },
    items: [{ id: 1, value: "one" }, { id: 2, value: "two" }],
  });
  const seen: string[] = [];
  createRenderEffect(() => {
    seen.push(value.user.name + ":" + value.user.score + ":" +
      value.items.map(item => item.value).join(","));
  });
  return { value, setValue, seen, dispose };
});
console.log("store", store.seen.join("|"));
store.setValue("user", "score", score => score + 1);
store.setValue("user", produce(user => { user.name = "Grace"; }));
console.log("store updates", store.seen.join("|"));
const first = store.value.items[0];
store.setValue("items", reconcile([{ id: 2, value: "TWO" }, { id: 1, value: "ONE" }]));
console.log("reconcile", store.seen.join("|"));
console.log("store identity", first === store.value.items[1]);
console.log("unwrap", unwrap(store.value).user.name);
store.dispose();
store.setValue("user", "score", 9);
console.log("store disposed", store.seen.length);

// An in-memory host keeps this audit independent of a display server. The
// actual Solid renderer drives these operations, including anchored moves.
interface HostNode {
  kind: string;
  value: string;
  children: HostNode[];
  parent: HostNode | undefined;
}
function hostNode(kind: string, value = ""): HostNode {
  return { kind, value, children: [], parent: undefined };
}
function remove(parent: HostNode, node: HostNode): void {
  const index = parent.children.indexOf(node);
  if (index < 0) throw new Error("removing a non-child");
  parent.children.splice(index, 1);
  node.parent = undefined;
}
const renderer = createRenderer<HostNode>({
  createElement: kind => hostNode(kind),
  createTextNode: value => hostNode("text", value),
  replaceText: (node, value) => { node.value = value; },
  setProperty: (node, name, value) => {
    if (name !== "value") throw new Error("unexpected property " + name);
    node.value = String(value);
  },
  insertNode(parent, node, anchor) {
    if (node === anchor) return;
    if (node.parent) remove(node.parent, node);
    const index = anchor ? parent.children.indexOf(anchor) : parent.children.length;
    if (index < 0) throw new Error("anchor is not a child");
    parent.children.splice(index, 0, node);
    node.parent = parent;
  },
  isTextNode: node => node.kind === "text",
  removeNode: remove,
  getParentNode: node => node.parent,
  getFirstChild: node => node.children[0],
  getNextSibling: node => node.parent?.children[node.parent.children.indexOf(node) + 1],
});

const root = hostNode("root");
const [rows, setRows] = createSignal(["a", "b", "c"]);
let creations = 0;
let rowCleanups = 0;
const disposeRows = renderer.render(() => createComponent(For, {
  get each() { return rows(); },
  children: item => {
    creations++;
    onCleanup(() => { rowCleanups++; });
    return renderer.createTextNode(item);
  },
}), root);
const originalA = root.children[0];
console.log("rows", root.children.map(node => node.value).join(","));
setRows(["c", "a", "b"]);
console.log("moved", root.children.map(node => node.value).join(","), root.children[1] === originalA, creations);
setRows(["b", "d"]);
console.log("replaced", root.children.map(node => node.value).join(","), creations, rowCleanups);
setRows([]);
console.log("empty", root.children.map(node => node.value).join(","), rowCleanups);
disposeRows();
setRows(["after disposal"]);
console.log("rows disposed", creations, rowCleanups);

const labelRoot = hostNode("root");
const [label, setLabel] = createSignal("before");
const disposeLabel = renderer.render(() => label, labelRoot);
const originalLabel = labelRoot.children[0];
setLabel("after");
console.log("text update", labelRoot.children[0].value, labelRoot.children[0] === originalLabel);
disposeLabel();
