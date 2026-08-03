// Entry point for the dependency-scale GC-rooting IR corpus.
//
// Compiled by scripts/gc_root_dominance_dep_corpus.sh for its IR, not for its
// stdout — see README.md in this directory. It is also runnable, and is run as
// the acceptance workload for the moving collector, so the printed line is
// deterministic on purpose.
import * as z from "../../node_modules/zod/src/index.js";
import "./alerts.js";
import "./orgs.js";
import "./scans.js";
import { allApiCalls, SCHEMAS, CALLBACKS } from "./shared.js";

// The #7280 library-only control: twenty lines of stock zod, no scaffolding.
// This is the program that failed 5/40 under the moving arm while the curated
// 25-file corpus passed 25/25.
function parseLoop(iterations: number): number {
  let ok = 0;
  for (let i = 0; i < iterations; i++) {
    const S = z
      .object({
        id: z.string().min(1).max(64),
        kind: z.string(),
        count: z.number().int(),
        ratio: z.number(),
        active: z.boolean(),
        labels: z.array(z.string()),
        meta: z.object({ note: z.string(), rank: z.number() }),
      })
      .array();
    const r = (S as unknown as { safeParse: (v: unknown) => { success: boolean } })
      .safeParse([
        {
          id: "id-" + i,
          kind: "k",
          count: i,
          ratio: i + 0.5,
          active: i % 2 === 0,
          labels: ["a", "b", "c"],
          meta: { note: "n" + i, rank: i },
        },
      ]);
    if (r.success) ok++;
  }
  return ok;
}

// The registry walk: cross-module closures created at module init, called back
// long after the frames that built them are gone.
function describeAll(): string[] {
  const calls = allApiCalls();
  const out: string[] = [];
  for (let i = 0; i < calls.length; i++) {
    out.push(calls[i].describe());
  }
  return out;
}

// Schemas built at module init, parsed later — the shape that keeps a library
// object live across many collections before it is finally dereferenced.
function parseRegistered(): number {
  let ok = 0;
  const keys: string[] = [];
  SCHEMAS.forEach((_v, k) => keys.push(k));
  keys.sort();
  for (let i = 0; i < keys.length; i++) {
    const schema = SCHEMAS.get(keys[i]) as {
      safeParse: (v: unknown) => { success: boolean };
    };
    const cb = CALLBACKS.get(keys[i]);
    const sample = {
      id: "row-" + i,
      kind: "k",
      count: i,
      ratio: 1.5,
      active: true,
      labels: ["x", "y"],
      severity: "high",
      meta: { note: "n", rank: i },
      slug: "org-" + i,
      seats: 4,
      owners: [{ login: "a", role: "admin" }],
      digest: "deadbeef",
      findings: [{ rule: "r", level: "warn", line: i }],
      summary: "s",
    };
    const r = schema.safeParse(Array.isArray(sample) ? sample : [sample]);
    const r2 = schema.safeParse(sample);
    if (r.success || r2.success) ok++;
    if (cb) cb(sample);
  }
  return ok;
}

const described = describeAll();
const parsed = parseLoop(96);
const registered = parseRegistered();

console.log("endpoints=" + described.length);
console.log("parsed=" + parsed);
console.log("registered=" + registered);
console.log(described[0]);
