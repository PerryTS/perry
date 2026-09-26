// commander: build a CLI definition and parse an argv vector per iteration.
import { Command } from "commander";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(5000, 200);
header("commander/parse_argv", "commander", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

function op(i: number, h: number): number {
  const program = new Command();
  program.name("tool").exitOverride()
    .option("-v, --verbose", "verbose output")
    .option("-p, --port <number>", "port", "3000")
    .option("-t, --tag <tags...>", "tags")
    .option("--no-color", "disable color")
    .argument("<file>", "input file")
    .argument("[rest...]", "extra");
  const argv = ["-v", "--port", String(8000 + (i % 100)), "--tag", "a", "b", "--no-color",
    "input-" + (i % 10) + ".txt", "x", "y"];
  program.parse(argv, { from: "user" });
  const o: any = program.opts();
  return fnv(h, String(o.verbose) + o.port + o.tag.join("/") + o.color + program.args.join(","));
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
