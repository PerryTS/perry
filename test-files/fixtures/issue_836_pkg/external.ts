// Issue #836 fixture: leaf re-exporter that the barrel will star-import.
// Mirrors zod's `node_modules/zod/src/v4/classic/external.ts` shape.

export { $ZodCheck, $ZodCheckStringFormat } from "./checks.ts";

export function makeSchema(name: string): { name: string } {
  return { name };
}
