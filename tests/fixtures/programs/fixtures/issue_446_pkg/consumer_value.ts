import { Inner } from "./inner.ts";

export function consumeValue(inner: Inner): string {
  const t = typeof (inner as any).setAdd;
  inner.setAdd(3, "value-import");
  return t;
}
