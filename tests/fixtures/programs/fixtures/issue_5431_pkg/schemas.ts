import * as core from "./core.ts";

// `export const X = /*@__PURE__*/ core.$constructor(...)` — zod's ZodString shape.
export const ZodString: any = /*@__PURE__*/ core.$constructor("ZodString", (inst: any) => {
  inst.kind = "string";
});

export function string(): any {
  return new ZodString({ type: "string" });
}
