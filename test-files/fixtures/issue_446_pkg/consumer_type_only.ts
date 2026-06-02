import type { Inner } from "./inner.ts";

export function consumeTypeOnly(inner: Inner): string {
  const t = typeof (inner as any).setAdd;
  inner.setAdd(1, "type-only");
  return t;
}
