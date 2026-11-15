// Many cells made in an init and dropped before the report: garbage.
function make(): number {
  const t: number[][] = [];
  for (let i = 0; i < 20000; i++) t.push([i, i + 1, i + 2]);
  return t.length;
}
export const n = make();
export const keep = [1, 2, 3];
