export const fromB: string = "b-export";

export async function loadA(): Promise<string> {
  const a = await import("./dynamic_import_cycle_a.ts");
  return a.fromA;
}
