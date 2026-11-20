import { type Inner } from "./inner.ts";

export function consumeInlineType(inner: Inner): string {
  const t = typeof (inner as any).setAdd;
  inner.setAdd(2, "inline-type");
  return t;
}
