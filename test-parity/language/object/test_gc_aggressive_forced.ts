// GC-aggression regression: forces gc() between every JSON parse,
// array op, and string op for 2,000 iterations. Must NOT crash;
// every value-flow assertion must hold across the forced collection.
//
// Catches regressions where:
//   - Stack-rooted JSValues are dropped during GC (conservative
//     scanner miss) and produce garbage on subsequent reads
//   - GC during JSON parse interleaves with the parse-arena suppress
//     mechanism (json.rs PARSE_GC_SUPPRESSED) incorrectly
//   - GC during write barrier (v0.5.224 path) leaves RS in a half-
//     populated state
//   - GC during lazy materialization wipes the sparse cache mid-walk
//   - GC during shadow-stack push/pop unbalances frame top
//   - GC during closure env init drops the env before the closure
//     is fully constructed

declare function gc(): void;

function hasGc(): boolean {
  return typeof gc === "function";
}

function makeData(i: number): any[] {
  const blob = '[{"a":' + i + ',"b":"v_' + i + '"},' +
               '{"a":' + (i + 1) + ',"b":"w"},' +
               '{"a":' + (i + 2) + ',"b":"z"}]';
  return JSON.parse(blob);
}

let acc = 0;
for (let i = 0; i < 2_000; i++) {
  const data = makeData(i);
  if (hasGc()) gc();

  acc += data[0].a;
  if (hasGc()) gc();

  // Force materialization of a lazy array via .map (allocates a new
  // tree, then GC immediately).
  const mapped = data.map((d: any) => d.a);
  if (hasGc()) gc();

  acc += mapped[0] + mapped[1] + mapped[2];

  // String concat across forced GC.
  const s = "iter_" + i + "_" + data[1].b;
  if (hasGc()) gc();
  acc += s.length;
}
console.log("done, acc=" + acc);
