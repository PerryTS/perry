// Re-export source for lib.ts (#7210 section 2's `ForeignVar`/`ForeignFunction`
// `NamespaceEntry` kinds — routed through a cross-module getter in
// `emit_namespace_populator`'s per-entry loop).

export const CHURN_TAG = "churn";

export function churnFromOther(seed: number): number {
  const bits: unknown[] = [];
  for (let i = 0; i < 500; i++) {
    bits.push({ i: i, s: "z" + i });
  }
  return seed + bits.length - 500;
}
