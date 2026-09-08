import assert from "node:assert/strict";
import { createSignal, onCleanup } from "solid-js";
import { createNativeRenderer, For, type NativeDriver, type ElementName, type NativeNode } from "../src/renderer.ts";

const widgets: { kind: ElementName; children: number[]; props: Record<string, unknown>; press: () => void }[] = [];
const operations: string[] = [];
const driver: NativeDriver = {
  create(kind, press) {
    widgets.push({ kind, children: [], props: {}, press });
    return widgets.length;
  },
  setProperty(handle, kind, name, value) { widgets[handle - 1].props[name] = value; },
  insert(parent, child, index, previousParent) {
    if (previousParent) {
      const old = widgets[previousParent - 1].children;
      old.splice(old.indexOf(child), 1);
    }
    widgets[parent - 1].children.splice(index, 0, child);
    operations.push(`insert ${child} ${index}`);
  },
  move(parent, from, to) {
    const children = widgets[parent - 1].children;
    const child = children.splice(from, 1)[0];
    children.splice(to, 0, child);
    operations.push(`move ${child} ${to}`);
  },
  remove(parent, child) {
    const children = widgets[parent - 1].children;
    children.splice(children.indexOf(child), 1);
    operations.push(`remove ${child}`);
  },
};
const renderer = createNativeRenderer(driver);
const { h } = renderer;
const root = driver.create("VStack", () => {});
const [count, setCount] = createSignal(0);
const [items, setItems] = createSignal(["a", "b", "c"]);
const [handler, setHandler] = createSignal<() => void>(() => setCount(n => n + 1));
let label: NativeNode;
let button: NativeNode;
let list: NativeNode;
let effects = 0;
let cleanups = 0;
const dispose = renderer.render(() => {
  onCleanup(() => cleanups++);
  label = h("Text", { get width() { return 100 + count(); } }, () => {
    effects++;
    return `Count ${count()}`;
  }) as NativeNode;
  button = h("Button", { get onPress() { return handler(); } }, "Increment") as NativeNode;
  list = h("VStack", null, For({
    get each() { return items(); },
    children: (item: string) => h("Text", null, item),
  })) as NativeNode;
  return h("VStack", null, label, button, list);
}, root);

assert.equal(widgets[label!.handle - 1].props.text, "Count 0");
assert.equal(widgets[button!.handle - 1].props.text, "Increment");
assert.equal(widgets.filter(w => w.kind === "Text").length, 4, "label text nodes allocate no extra widgets");
const labelHandle = label!.handle;
widgets[button!.handle - 1].press();
assert.equal(widgets[labelHandle - 1].props.text, "Count 1");
assert.equal(widgets[labelHandle - 1].props.width, 101);
setHandler(() => () => setCount(n => n + 10));
widgets[button!.handle - 1].press();
assert.equal(widgets[labelHandle - 1].props.text, "Count 11");
assert.equal(label!.handle, labelHandle);

const original = [...widgets[list!.handle - 1].children];
setItems(rows => [rows[2], rows[0], rows[1]]);
assert.deepEqual(widgets[list!.handle - 1].children, [original[2], original[0], original[1]]);
setItems(["b", "d", "c"]);
const final = widgets[list!.handle - 1].children;
assert.equal(final[0], original[1]);
assert.equal(final[2], original[2]);
assert.equal(widgets[final[1] - 1].props.text, "d");
assert.equal(list!.children[0].parent, list!);
assert.ok(operations.some(op => op.startsWith("move ")));

const other = renderer.createElement("VStack");
const moved = list!.children[0];
renderer.insertNode(other, moved);
assert.equal(moved.parent, other);
assert.deepEqual(widgets[other.handle - 1].children, [moved.handle]);
assert.ok(!widgets[list!.handle - 1].children.includes(moved.handle));
assert.throws(() => renderer.insertNode(other, list!, list!.children[0]));
assert.throws(() => renderer.insertNode(moved, other));
assert.throws(() => renderer.insertNode(list!, list!));
renderer.insertNode(list!, moved, list!.children[0]);
assert.equal(widgets[list!.handle - 1].children[0], moved.handle);
assert.deepEqual(widgets[other.handle - 1].children, []);

const beforeDispose = effects;
dispose();
dispose();
setCount(99);
widgets[button!.handle - 1].press();
assert.equal(count(), 99, "disposed owners release native user callbacks");
assert.equal(cleanups, 1);
assert.equal(effects, beforeDispose);
assert.deepEqual(widgets[root - 1].children, []);
// Perry widget handles can use NaN-boxed words. They are opaque tokens;
// numeric truthiness must never decide whether a native widget exists.
const opaqueWrites: string[] = [];
const opaque = createNativeRenderer({
  create() { return Number.NaN; },
  setProperty(_handle, _kind, name, value) {
    if (name === "text") opaqueWrites.push(String(value));
  },
  insert() {}, move() {}, remove() {},
});
const [raw, setRaw] = createSignal("raw 0");
let rawContainer: NativeNode;
const disposeOpaque = opaque.render(() => {
  rawContainer = opaque.h("VStack", null, raw);
  return rawContainer;
}, Number.NaN);
const rawNode = rawContainer!.children[0];
assert.equal(opaqueWrites[opaqueWrites.length - 1], "raw 0");
setRaw("raw 1");
assert.equal(opaqueWrites[opaqueWrites.length - 1], "raw 1");
assert.equal(rawContainer!.children[0], rawNode, "single reactive text preserves its native node");
disposeOpaque();
console.log("PASS Solid native renderer: signals, properties, events, keyed order, reparenting, disposal");
