// Run under auto, forced tape and forced direct modes with the same output.
function nested(depth: number, leaf: string): string {
  return "[".repeat(depth) + leaf + "]".repeat(depth);
}

function inspect(text: string, depth: number): number {
  let value: any = JSON.parse(text);
  let count = 0;
  for (let i = 0; i < depth; i++) {
    if (!Array.isArray(value) || value.length !== 1) throw new Error("bad array");
    value = value[0];
    count++;
  }
  return count + value;
}

for (const depth of [999, 1000, 1001, 10000]) {
  console.log("depth", depth, inspect(nested(depth, "7"), depth));
}
const quoted = JSON.parse('[{"s":"[[{}]","a":[]}]');
console.log("quoted", quoted[0].s, quoted[0].a.length);
for (const text of [nested(1001, "01"), nested(1001, "0") + "x", '[?,[' + "[".repeat(1200)]) {
  try {
    JSON.parse(text);
    console.log("invalid accepted");
  } catch (error: any) {
    console.log("invalid", error.name);
  }
}
// A forced oversized tape keeps its preflight before reserving native storage.
console.log("oversized", inspect(" ".repeat(16 * 1024 * 1024 + 1) + nested(1001, "9"), 1001));
