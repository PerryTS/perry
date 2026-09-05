import {
  VStack, HStack, Text, Button, Spacer, Divider,
  textSetString, buttonSetTitle, textSetFontSize,
  widgetAddChildAt, widgetRemoveChild, widgetReorderChild,
  widgetSetWidth, widgetSetHeight, widgetSetOpacity,
  widgetSetHidden, widgetSetEnabled, widgetSetTooltip,
  widgetSetBackgroundColor, setCornerRadius, setPadding,
  type Widget,
} from "perry/ui";
import { createNativeRenderer, type NativeDriver, type ElementName } from "./renderer.ts";
export type { NativeNode, Child, Component, Props, ElementName } from "./renderer.ts";
export { For } from "./renderer.ts";

// Perry injects this target constant (0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux).
declare const __platform__: number;

function numeric(value: unknown, fallback: number): number {
  if (value == null) return fallback;
  if (typeof value !== "number" || !Number.isFinite(value)) throw new Error("Expected a finite native widget value");
  return value;
}

const driver: NativeDriver = {
  create(kind: ElementName, onPress: () => void): number {
    switch (kind) {
      case "VStack": return VStack(8, []);
      case "HStack": return HStack(8, []);
      case "Text": return Text("");
      case "Button": return Button("", onPress);
      case "Spacer": return Spacer();
      case "Divider": return Divider();
    }
  },
  setProperty(handle, kind, name, value) {
    const widget = handle as Widget;
    switch (name) {
      case "text":
        if (kind === "Text") textSetString(widget, value == null ? "" : String(value));
        else if (kind === "Button") buttonSetTitle(widget, value == null ? "" : String(value));
        else throw new Error(`text is unsupported on ${kind}`);
        return;
      case "width": widgetSetWidth(widget, numeric(value, 0)); return;
      case "height": widgetSetHeight(widget, numeric(value, 0)); return;
      case "opacity": widgetSetOpacity(widget, numeric(value, 1)); return;
      case "hidden": widgetSetHidden(widget, value ? 1 : 0); return;
      case "disabled": widgetSetEnabled(widget, value ? 0 : 1); return;
      case "tooltip": widgetSetTooltip(widget, value == null ? "" : String(value)); return;
      case "cornerRadius": setCornerRadius(widget, numeric(value, 0)); return;
      case "padding": {
        const amount = numeric(value, 0);
        setPadding(widget, amount, amount, amount, amount);
        return;
      }
      case "fontSize":
        if (kind !== "Text") throw new Error("fontSize is supported on Text");
        textSetFontSize(widget, numeric(value, 13));
        return;
      case "backgroundColor": {
        const color = value == null ? [0, 0, 0, 0] : value;
        if (!Array.isArray(color) || color.length !== 4) throw new Error("backgroundColor expects [r, g, b, a]");
        widgetSetBackgroundColor(widget, numeric(color[0], 0), numeric(color[1], 0), numeric(color[2], 0), numeric(color[3], 0));
        return;
      }
      default: throw new Error(`Unsupported Perry Solid property: ${name}`);
    }
  },
  insert(parent, child, index, previousParent) {
    // AppKit's indexed insertion detaches without destroying retained layout
    // metadata. Other backends need the old parent explicitly cleared first.
    if (previousParent !== null && __platform__ !== 0) {
      widgetRemoveChild(previousParent as Widget, child as Widget);
    }
    widgetAddChildAt(parent as Widget, child as Widget, index);
  },
  move(parent, from, to) { widgetReorderChild(parent as Widget, from, to); },
  remove(parent, child) { widgetRemoveChild(parent as Widget, child as Widget); },
};

const native = createNativeRenderer(driver);
export const h = native.h;
export const render = native.render;
export const createElement = native.createElement;
export const createTextNode = native.createTextNode;
export const insert = native.insert;
export const spread = native.spread;
export const setProp = native.setProp;
export const createComponent = native.createComponent;
export const effect = native.effect;
export const memo = native.memo;
export const mergeProps = native.mergeProps;
export const use = native.use;
