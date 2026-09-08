import { createRenderer } from "solid-js/universal";
import { createRoot, getOwner, onCleanup, mergeProps, For as SolidFor, type Accessor } from "solid-js";

export type ElementName = "VStack" | "HStack" | "Text" | "Button" | "Spacer" | "Divider";
export type Props = Record<string, unknown>;
export type Child = NativeNode | string | number | boolean | null | undefined | Child[] | (() => Child);
export type Component<P = Props> = (props: P) => Child;

/** Solid For with native children instead of DOM-specific JSX declarations. */
export const For = SolidFor as <T>(props: {
  each: readonly T[] | false | null | undefined;
  fallback?: Child;
  children: (item: T, index: Accessor<number>) => Child;
}) => Child;

/** Backend operations. Handles belong to the native widget registry. */
export interface NativeDriver {
  create(kind: ElementName, onPress: () => void): number;
  setProperty(handle: number, kind: ElementName, name: string, value: unknown, previous: unknown): void;
  insert(parent: number, child: number, index: number, previousParent: number | null): void;
  move(parent: number, from: number, to: number): void;
  remove(parent: number, child: number): void;
}

/** Retained ordering metadata; native handles themselves have no sibling API. */
export interface NativeNode {
  kind: ElementName | "#text" | "#root";
  handle: number;
  materialized: boolean;
  parent: NativeNode | null;
  children: NativeNode[];
  props: Props;
  text: string;
}

function isLabel(node: NativeNode): boolean {
  return node.kind === "Text" || node.kind === "Button";
}

function isContainer(node: NativeNode): boolean {
  return node.kind === "VStack" || node.kind === "HStack" || node.kind === "#root";
}

export function createNativeRenderer(driver: NativeDriver) {
  function makeNode(kind: NativeNode["kind"], text = ""): NativeNode {
    return { kind, handle: 0, materialized: false, parent: null, children: [], props: {}, text };
  }

  function createElement(name: string): NativeNode {
    // JSX intrinsic names are lowercase; the native driver uses Perry names.
    switch (name) {
      case "vstack": name = "VStack"; break;
      case "hstack": name = "HStack"; break;
      case "text": name = "Text"; break;
      case "button": name = "Button"; break;
      case "spacer": name = "Spacer"; break;
      case "divider": name = "Divider"; break;
    }
    if (!["VStack", "HStack", "Text", "Button", "Spacer", "Divider"].includes(name)) {
      throw new Error(`Unsupported Perry Solid element: ${name}`);
    }
    const node = makeNode(name as ElementName);
    // A native button's dispatcher outlives a removed Solid owner. Release
    // user callbacks when that owner is disposed, even before native detach.
    if (getOwner()) onCleanup(() => { node.props = {}; });
    node.handle = driver.create(name as ElementName, () => {
      const callback = node.props.onPress;
      if (typeof callback === "function") callback();
    });
    node.materialized = true;
    return node;
  }

  function materialize(node: NativeNode): number {
    // Text under a Text/Button contributes to its label. Allocate an independent
    // native Text only when the text node is actually inserted into a container.
    if (node.kind === "#text" && !node.materialized) {
      node.handle = driver.create("Text", () => {});
      node.materialized = true;
      driver.setProperty(node.handle, "Text", "text", node.text, undefined);
    }
    return node.handle;
  }

  function refreshLabel(node: NativeNode): void {
    let text = "";
    for (const child of node.children) text += child.text;
    driver.setProperty(node.handle, node.kind as ElementName, "text", text, undefined);
  }

  function removeNode(parent: NativeNode, node: NativeNode): void {
    if (node.parent !== parent) return;
    const index = parent.children.indexOf(node);
    parent.children.splice(index, 1);
    node.parent = null;
    if (isLabel(parent)) refreshLabel(parent);
    else driver.remove(parent.handle, materialize(node));
  }

  function insertNode(parent: NativeNode, node: NativeNode, anchor?: NativeNode): void {
    if (anchor === node) return;
    if (anchor && anchor.parent !== parent) throw new Error("Insertion anchor belongs to another parent");
    if (isLabel(parent)) {
      if (node.kind !== "#text") throw new Error("Text and Button children must be text");
    } else if (!isContainer(parent)) {
      throw new Error(`${parent.kind} cannot contain children`);
    }
    for (let ancestor: NativeNode | null = parent; ancestor; ancestor = ancestor.parent) {
      if (ancestor === node) throw new Error("Cannot insert a node into its own subtree");
    }
    const previousParent = node.parent;
    const previousIndex = previousParent ? previousParent.children.indexOf(node) : -1;
    if (previousParent) previousParent.children.splice(previousIndex, 1);
    const index = anchor ? parent.children.indexOf(anchor) : parent.children.length;
    parent.children.splice(index, 0, node);
    node.parent = parent;

    if (previousParent && previousParent !== parent && isLabel(previousParent)) refreshLabel(previousParent);
    if (isLabel(parent)) {
      if (previousParent && !isLabel(previousParent)) driver.remove(previousParent.handle, materialize(node));
      refreshLabel(parent);
    } else if (previousParent === parent) {
      if (previousIndex !== index) driver.move(parent.handle, previousIndex, index);
    } else {
      const oldHandle = previousParent && !isLabel(previousParent) ? previousParent.handle : null;
      driver.insert(parent.handle, materialize(node), index, oldHandle);
    }
  }

  const renderer = createRenderer<NativeNode>({
    createElement,
    createTextNode(value) { return makeNode("#text", String(value)); },
    isTextNode(node) { return node.kind === "#text"; },
    replaceText(node, value) {
      node.text = String(value);
      if (node.materialized) driver.setProperty(node.handle, "Text", "text", node.text, undefined);
      if (node.parent && isLabel(node.parent)) refreshLabel(node.parent);
    },
    setProperty(node, name, value, previous) {
      if (name === "onPress") {
        if (node.kind !== "Button") throw new Error("onPress is supported on Button");
        if (value != null && typeof value !== "function") throw new Error("onPress must be a function");
      } else {
        driver.setProperty(node.handle, node.kind as ElementName, name, value, previous);
      }
      node.props[name] = value;
    },
    insertNode,
    removeNode,
    getParentNode(node) { return node.parent || undefined; },
    getFirstChild(node) { return node.children[0]; },
    getNextSibling(node) {
      const parent = node.parent;
      return parent ? parent.children[parent.children.indexOf(node) + 1] : undefined;
    },
  });

  // Solid's implementation accepts arrays, primitives and accessors too;
  // its universal declaration narrows component results to NodeType.
  const createComponent = renderer.createComponent as <P>(component: (props: P) => Child, props: P) => Child;

  /** Native hyperscript. Reactive properties use getters; children may be accessors. */
  function h(type: ElementName, props?: Props | null, ...children: Child[]): NativeNode;
  function h<P>(type: Component<P>, props: P, ...children: Child[]): Child;
  function h(type: ElementName | Component<any>, props: Props | null = null, ...children: Child[]): Child {
    const properties = children.length
      ? mergeProps(props || {}, { children: children.length === 1 ? children[0] : children })
      : (props || {});
    if (typeof type === "function") return createComponent(type, properties);
    const node = createElement(type);
    renderer.spread(node, properties);
    return node;
  }

  function releaseSubtree(node: NativeNode): void {
    for (const child of node.children) releaseSubtree(child);
    node.children = [];
    node.parent = null;
    node.props = {};
  }

  /** Mount into an existing native stack; dispose effects and detach its nodes. */
  function render(code: () => Child, handle: number): () => void {
    const root = makeNode("#root");
    root.handle = handle;
    root.materialized = true;
    const dispose = createRoot(dispose => {
      renderer.insert(root, code());
      return dispose;
    });
    let disposed = false;
    return () => {
      if (disposed) return;
      disposed = true;
      dispose();
      while (root.children.length) {
        const child = root.children[root.children.length - 1];
        removeNode(root, child);
        releaseSubtree(child);
      }
    };
  }

  return { ...renderer, createComponent, render, h, removeNode };
}
