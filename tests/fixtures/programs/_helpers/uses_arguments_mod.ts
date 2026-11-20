// Helper for test_gap_cross_module_arguments.ts (#1816): an exported function
// that references `arguments` (→ synthetic-arguments rest param).
export function variadicSum(): number {
  let total = 0;
  for (let i = 0; i < arguments.length; i++) total += arguments[i] as number;
  return total;
}
export function firstOrCount(a: number, b?: number): number {
  return arguments.length === 1 ? a : a + (b as number);
}
