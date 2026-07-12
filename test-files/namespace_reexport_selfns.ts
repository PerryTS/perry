// Helper for test_gap_namespace_reexport_barrel.ts — issue #5916.
//
// Declares a namespace-valued export by re-exporting ITSELF under a name
// (`export * as Token from "./this-file"`). This is the shape Effect / zod
// barrels use pervasively, and the value of `Token` is a module NAMESPACE, not
// an ordinary function or const.
export * as Token from "./namespace_reexport_selfns.ts";

const CHARS_PER_TOKEN = 4;

export const estimate = (input: string): number =>
  Math.max(0, Math.round(input.length / CHARS_PER_TOKEN));
