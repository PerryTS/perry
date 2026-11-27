// util.format's %d is Number(value), so a BigInt from valueOf or a BigInt
// wrapper converts, while %i and %f are parseInt and parseFloat of the
// value's string form. A primitive BigInt keeps its `n` for %d and %i.
import { format } from "node:util";

const values: any[] = [
  1n,
  { valueOf: () => 10n },
  Object(1n),
  { valueOf: () => 7 },
  { toString: () => "8.5" },
  "9.5x",
  "0x10",
  "",
  true,
  null,
  undefined,
  [3],
  -0.5,
  1e21,
  Symbol("s"),
];
for (const spec of ["%d", "%i", "%f"]) {
  process.stdout.write(`${spec.slice(1)}: ${values.map((v) => format(spec, v)).join(" | ")}\n`);
}
