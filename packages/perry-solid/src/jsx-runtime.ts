import type { Child, NativeNode, Props } from "./renderer.ts";

/** Types for JSX preserved for Perry's Solid compiler mode. */
export namespace JSX {
  export type Element = Child;
  export type ElementType = keyof IntrinsicElements | ((props: any) => Child);
  export interface ElementChildrenAttribute { children: {}; }
  export interface NativeProps extends Props {
    children?: Child;
    ref?: NativeNode | ((node: NativeNode) => void);
    text?: string;
    onPress?: () => void;
    width?: number;
    height?: number;
    opacity?: number;
    hidden?: boolean;
    disabled?: boolean;
    tooltip?: string;
    padding?: number;
    cornerRadius?: number;
    fontSize?: number;
    backgroundColor?: [number, number, number, number];
  }
  export interface IntrinsicElements {
    vstack: NativeProps;
    hstack: NativeProps;
    text: NativeProps;
    button: NativeProps;
    spacer: NativeProps;
    divider: NativeProps;
  }
}
