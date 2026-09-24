// Loads cyc_holder.ts lazily and never calls Holder.k() itself.
console.log("cyc_lib init");
export const config = { n: 5 };
export async function loadHolder(): Promise<number> {
  const m = await import("./cyc_holder.ts");
  return new m.Holder().k();
}
