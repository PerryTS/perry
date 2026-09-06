import assert from "node:assert/strict";
import { createSignal, onCleanup } from "solid-js";
import { For, type NativeNode, type Child } from "../../src/renderer.ts";
import { render, root, widgets, props, children } from "./host.ts";

// Deliberately collide with the transform's first candidate helper prefix.
const __perry_solid_0_createElement = "user binding";
const [count, setCount] = createSignal(0);
const [rows, setRows] = createSignal(["Alpha", "Beta", "Gamma"]);
const [shown, setShown] = createSignal(1);
const [spreadProps, setSpreadProps] = createSignal({ text: "Spread 0", width: 150 });
const [handler, setHandler] = createSignal<() => void>(() => setCount(value => value + 1));
let label!: NativeNode;
let button!: NativeNode;
let list!: NativeNode;
let raw!: NativeNode;
let conditional!: NativeNode;
let spreadOnly!: NativeNode;
let precedence!: NativeNode;
const memberRef: { current?: NativeNode } = {};
let callbackRef!: NativeNode;
let refCalls = 0;
const capture = (node: NativeNode) => { count(); refCalls++; callbackRef = node; };
let componentRuns = 0;
let cleanups = 0;

function Panel(properties: { children?: Child; title: string }) {
  componentRuns++;
  return <vstack tooltip={properties.title}>{properties.children}</vstack>;
}
const UI = { Panel };
const dispose = render(() => {
  onCleanup(() => cleanups++);
  return <UI.Panel title={"Panel " + count()}>
    <text ref={label} width={100 + count()}>Count: {count()}</text>
    <button ref={button} onPress={handler()}>Increment</button>
    <vstack ref={raw}>{"Raw: " + count()}</vstack>
    <vstack ref={list}>
      <For each={rows()}>{item => <text>{item}</text>}</For>
    </vstack>
    <vstack>{shown() > 0 && <text ref={conditional}>Kept</text>}</vstack>
    <text ref={spreadOnly} {...spreadProps()} />
    <text {...spreadProps()} width={200 + count()} ref={precedence} />
    <text ref={memberRef.current}>Member</text>
    <text ref={capture}>Callback</text>
    <><text>Fragment</text><divider /></>
  </UI.Panel>;
}, root);

assert.equal(__perry_solid_0_createElement, "user binding");
assert.equal(props(label!).text, "Count: 0");
assert.equal(props(label!).width, 100);
assert.equal(props(button!).text, "Increment");
assert.equal(props(spreadOnly!).text, "Spread 0");
assert.equal(props(precedence!).width, 200);
assert.equal(props(memberRef.current!).text, "Member");
assert.equal(props(callbackRef!).text, "Callback");
const firstRaw = children(raw!)[0];
const firstConditional = conditional!;
const firstRows = [...children(list!)];
widgets[button!.handle - 1].press();
assert.equal(props(label!).text, "Count: 1");
assert.equal(props(label!).width, 101);
assert.equal(children(raw!)[0], firstRaw);
assert.equal(widgets[firstRaw - 1].props.text, "Raw: 1");
assert.equal(componentRuns, 1, "signal writes do not rerun the component");
setHandler(() => () => setCount(value => value + 10));
widgets[button!.handle - 1].press();
assert.equal(props(label!).text, "Count: 11");
assert.equal(refCalls, 1, "ref callbacks do not subscribe to signals they read");
setRows(items => [items[2], items[0], items[1]]);
assert.deepEqual(children(list!), [firstRows[2], firstRows[0], firstRows[1]]);
setShown(2);
assert.equal(conditional!, firstConditional, "truthy condition updates preserve the native branch");
setShown(0);
assert.equal(firstConditional.parent, null);
setShown(1);
assert.notEqual(conditional!, firstConditional);
setSpreadProps({ text: "Spread 1", width: 160 });
assert.equal(props(spreadOnly!).text, "Spread 1");
assert.equal(props(spreadOnly!).width, 160);
assert.equal(props(precedence!).width, 211, "later attributes retain precedence over a changing spread");
const beforeDispose = props(label!).text;
dispose();
setCount(99);
widgets[button!.handle - 1].press();
assert.equal(count(), 99);
assert.equal(props(label!).text, beforeDispose);
assert.equal(cleanups, 1);
assert.deepEqual(widgets[root - 1].children, []);
console.log("PASS Solid JSX: native updates, components, keyed identity, conditionals, spreads, refs, fragments, disposal");
