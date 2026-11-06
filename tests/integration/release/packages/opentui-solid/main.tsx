import { createSignal, For, Show, Switch, Match, onCleanup } from "solid-js";
import { createStore } from "solid-js/store";
import { render, effect, makeNode, content, expect, type Node } from "./host.ts";

const [count, setCount] = createSignal(0);
const [handler, setHandler] = createSignal(() => setCount(count() + 1));
const [visible, setVisible] = createSignal(1);
const [items, setItems] = createSignal(["A", "B"]);
const [spreadProps, setSpreadProps] = createSignal({ width: 10, title: "first" });
const [store, setStore] = createStore({ title: "store 0" });
let plain = "initial";
let native!: Node;
let branch!: Node;
let spreadNode!: Node;
let component!: Node;
let forwarded!: Node;
let typedRef!: Node;
const member: { current?: Node } = {};
let callback!: Node;
let refs = 0;
let componentRuns = 0;
let directives = 0;
let directiveValue = -1;
let cleanups = 0;
const capture = (node: Node) => { count(); callback = node; refs++; };
function focus(node: Node, accessor: () => number) {
  directives++;
  effect(() => { directiveValue = accessor(); });
  onCleanup(() => cleanups++);
}
function Panel(props: any) {
  componentRuns++;
  return <box ref={component} label={props.label} plain={props.plain}>{props.children}</box>;
}
function Forward(props: any) { return <text ref={props.ref}>forwarded</text>; }
const root = makeNode("root");
const dispose = render(() => <box>
  <text ref={native} width={count() + 1} plain={plain} onPress={handler()} use:focus={count()}>
    before {count()}<text> middle </text>{count() + 1} after
  </text>
  <Panel label={store.title} plain={plain}><text>child {count()}</text></Panel>
  <text ref={spreadNode} width={0} {...spreadProps()} title={"last " + count()} />
  <text ref={member.current}>member</text>
  <text ref={capture}>callback</text>
  <Forward ref={forwarded} />
  <text ref={typedRef!}>typed</text>
  <text ref={null} />
  <box>{visible() && <text ref={branch}>branch</text>}</box>
  <For each={items()}>{item => <text>{item}</text>}</For>
  <Show when={count() > 0} fallback={<text>zero</text>}><text>positive</text></Show>
  <Switch><Match when={count() === 0}><text>switch 0</text></Match><Match when={count() > 0}><text>switch 1</text></Match></Switch>
  <>{"fragment "}{count()}</>
</box>, root);
expect(content(native), "before 0 middle 1 after", "initial mixed children");
expect(native.props.width, 1, "initial dynamic prop");
expect(content(member.current!), "member", "member ref");
expect(content(callback), "callback", "callback ref");
expect(content(forwarded), "forwarded", "component ref");
expect(content(typedRef), "typed", "non-null ref");
const firstBranch = branch;
plain = "changed";
native.props.onPress();
setStore("title", "store 1");
expect(content(native), "before 1 middle 2 after", "updated insertion ranges");
expect(native.props.width, 2, "updated dynamic prop");
expect(native.props.plain, "initial", "plain identifier stays static");
expect(component.props.plain, "initial", "component identifier stays static");
expect(component.props.label, "store 1", "store uses client build");
expect(content(component), "child 1", "component children getter");
expect(componentRuns, 1, "component does not rerun");
expect(directives, 1, "directive runs once");
expect(directiveValue, 1, "directive accessor tracks");
expect(refs, 1, "ref callback runs untracked");
setSpreadProps({ width: 20, title: "second" });
expect(spreadNode.props.width, 20, "spread update");
expect(spreadNode.props.title, "last 1", "attribute after spread wins");
setVisible(2);
expect(branch, firstBranch, "truthy branch identity");
setVisible(0);
expect(firstBranch.parent, null, "branch removed");
setVisible(1);
expect(branch === firstBranch, false, "branch recreated");
setItems(["B", "A"]);
expect(content(root).includes("BApositiveswitch 1fragment 1"), true, "control flow and fragment update");
setHandler(() => () => setCount(count() + 10));
native.props.onPress();
expect(count(), 11, "event handler prop updates");
const before = content(native);
dispose();
setCount(9);
expect(content(native), before, "disposed effects stay stopped");
expect(cleanups, 1, "directive cleanup");
console.log("PASS universal JSX: effects, children, components, refs, directives, spreads, fragments, control flow, client core/store");
