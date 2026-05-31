// Gap test: Array dynamic dispatch completeness (#2803, #2808).
//
// Exercises ES2023 immutable methods (toReversed / toSorted / toSpliced) and
// toLocaleString through DYNAMIC dispatch — `(arr as any).method()` and the
// computed `arr[m]()` form — so the runtime dispatch tower handles them, not
// just the static codegen fast paths. Output must be byte-identical to
// `node --experimental-strip-types`.

// --- #2803: toReversed / toSorted / toSpliced via `as any` ---
const base = [3, 1, 2];

{
  const a = base.slice();
  const out = (a as any).toReversed();
  console.log("toReversed: " + JSON.stringify([out, a, out === a]));
}
{
  const a = base.slice();
  const out = (a as any).toSorted();
  console.log("toSorted default: " + JSON.stringify([out, a, out === a]));
}
{
  const a = base.slice();
  const out = (a as any).toSorted((x: number, y: number) => x - y);
  console.log("toSorted comparator: " + JSON.stringify([out, a, out === a]));
}
{
  const a = [1, 2, 3, 4];
  const out = (a as any).toSpliced(1, 2, 9);
  console.log("toSpliced: " + JSON.stringify([out, a, out === a]));
}
{
  // toSpliced(start) deletes through the end.
  const a = [1, 2, 3, 4];
  const out = (a as any).toSpliced(2);
  console.log("toSpliced start-only: " + JSON.stringify([out, a]));
}

// --- #2803 via computed-key dispatch `arr[m]()` ---
{
  const a = [5, 1, 4, 2, 3];
  const m = "toSorted";
  const out = (a as any)[m]((x: number, y: number) => x - y);
  console.log("computed toSorted: " + JSON.stringify(out));
}
{
  const a = [10, 20, 30];
  const m = "toReversed";
  const out = (a as any)[m]();
  console.log("computed toReversed: " + JSON.stringify(out));
}

// --- #2808: Array.prototype.toLocaleString element dispatch ---
const obj = {
  toLocaleString(locales: any, options: any) {
    return `obj:${locales}:${options && options.tag}`;
  },
};
{
  const r = ([1, null, undefined, "x"] as any).toLocaleString();
  console.log("toLocaleString plain: " + JSON.stringify(r));
}
{
  const r = ([obj] as any).toLocaleString("xx-YY", { tag: "opt" });
  console.log("toLocaleString forwarded: " + JSON.stringify(r));
}
{
  const r = ([1, 2, 3] as any).toLocaleString();
  console.log("toLocaleString numbers: " + JSON.stringify(r));
}
{
  // empty array -> empty string
  const r = ([] as any).toLocaleString();
  console.log("toLocaleString empty: " + JSON.stringify(r));
}
{
  // number element with locale: grouping separators.
  const r = ([1000.5] as any).toLocaleString("en-US");
  console.log("toLocaleString number locale: " + JSON.stringify(r));
}
{
  // Date element dispatches to Date.prototype.toLocaleString (the element's
  // own method is invoked, producing a date+time string rather than
  // "[object Date]"). The exact wall-clock value tracks the host timezone, so
  // assert the dispatch shape (a non-empty, comma-bearing string) rather than
  // a fixed instant to keep the test deterministic across timezones.
  const r = ([new Date(Date.UTC(2020, 0, 2))] as any).toLocaleString();
  console.log("toLocaleString date dispatched: " + (r.length > 0 && r.includes(",")));
}
