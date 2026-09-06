import { createNativeRenderer, type NativeDriver, type NativeNode, type ElementName } from "../../src/renderer.ts";

export const widgets: {
  kind: ElementName;
  children: number[];
  props: Record<string, unknown>;
  press: () => void;
}[] = [];

const driver: NativeDriver = {
  create(kind, press) {
    widgets.push({ kind, children: [], props: {}, press });
    return widgets.length;
  },
  setProperty(handle, _kind, name, value) { widgets[handle - 1].props[name] = value; },
  insert(parent, child, index, previousParent) {
    if (previousParent !== null) {
      const old = widgets[previousParent - 1].children;
      old.splice(old.indexOf(child), 1);
    }
    widgets[parent - 1].children.splice(index, 0, child);
  },
  move(parent, from, to) {
    const children = widgets[parent - 1].children;
    const child = children.splice(from, 1)[0];
    children.splice(to, 0, child);
  },
  remove(parent, child) {
    const children = widgets[parent - 1].children;
    children.splice(children.indexOf(child), 1);
  },
};

export const root = driver.create("VStack", () => {});
export const {
  render, h, createElement, createTextNode, createComponent, insert, insertNode,
  spread, setProp, effect, memo, mergeProps, use,
} = createNativeRenderer(driver);

export function props(node: NativeNode): Record<string, unknown> {
  return widgets[node.handle - 1].props;
}
export function children(node: NativeNode): number[] {
  return widgets[node.handle - 1].children;
}
