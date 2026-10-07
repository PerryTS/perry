// One agent's run: a receiver grows through the first two keys of a seeded
// literal and dies, collections reap its keys backing, then the literal's
// module initializes and allocates.
const names = ["ssw_p", "ssw_q", "ssw_x"];

function grow(i: number): number {
  const o: any = {};
  for (const k of names) o[k] = i;
  return Object.keys(o).length;
}

function churn(): number {
  let keep: any[] = [];
  let total = 0;
  for (let i = 0; i < 300000; i++) {
    keep.push({ i });
    if (keep.length > 1000) {
      total += keep.length;
      keep = [];
    }
  }
  return total;
}

export async function run(label: string): Promise<string> {
  let n = 0;
  for (let i = 0; i < 3; i++) n += grow(i);
  churn();
  const g = (globalThis as any).gc;
  if (typeof g === "function") {
    g();
    g();
  }
  const m = await import("./static_seed_prefix_lit.ts");
  const a = m.make(1);
  const b = m.make(200);
  return `${label} grown=${n} a=${JSON.stringify(a)} b=${Object.keys(b).join(",")}`;
}
