import { createRenderer } from "solid-js/universal";

export type Node = { tag: string; text: string; props: any; parent: Node | null; children: Node[] };
export function makeNode(tag: string, text = ""): Node {
  return { tag, text, props: {}, parent: null, children: [] };
}
const renderer = createRenderer<Node>({
  createElement: tag => makeNode(tag),
  createTextNode: text => makeNode("#text", text),
  isTextNode: node => node.tag === "#text",
  replaceText: (node, text) => { node.text = text; },
  setProperty: (node, key, value) => { node.props[key] = value; },
  insertNode(parent, node, marker) {
    if (node === marker) return;
    if (node.parent) {
      const old = node.parent.children.indexOf(node);
      node.parent.children.splice(old, 1);
    }
    const index = marker ? parent.children.indexOf(marker) : parent.children.length;
    parent.children.splice(index, 0, node);
    node.parent = parent;
  },
  removeNode(parent, node) {
    parent.children.splice(parent.children.indexOf(node), 1);
    node.parent = null;
  },
  getParentNode: node => node.parent,
  getFirstChild: node => node.children[0],
  getNextSibling: node => node.parent?.children[node.parent.children.indexOf(node) + 1],
});
export const { createElement, createTextNode, insertNode, insert, setProp, spread,
  createComponent, effect, memo, mergeProps, use, render } = renderer;

export function content(node: Node): string {
  if (node.tag === "#text") return node.text;
  return node.children.map(content).join("");
}
export function expect(actual: any, expected: any, label: string) {
  if (actual !== expected) throw new Error(label + ": " + String(actual) + " != " + String(expected));
}
