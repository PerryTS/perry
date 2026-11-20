// Array.prototype.flat per ECMAScript FlattenIntoArray: holes are absent
// (HasProperty is false) and are skipped — not copied as null — at every
// depth, including flat(0). `undefined` / `null` elements are real and kept.
// Non-array elements (plain objects, etc.) are pushed as-is, never flattened
// (previously a plain-object element segfaulted: its bytes were read as an
// ArrayHeader length).

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

// Holes skipped at depth 1.
show("hole d1", JSON.stringify([1, , 3].flat()));
show("nested hole", JSON.stringify([1, , [3, , 4]].flat()));
show("only holes", JSON.stringify([, , ,].flat()));
show("trailing hole", JSON.stringify([[1, ,], [, 2]].flat()));

// Deeper depths and Infinity also skip holes.
show("d2", JSON.stringify([1, [2, , [3, , 4]]].flat(2)));
show("inf", JSON.stringify([1, , [2, , [3, , 4]]].flat(Infinity)));
show("d0", JSON.stringify([1, , 3].flat(0)));

// undefined / null are kept; only holes are removed.
show("undefined kept", JSON.stringify([1, undefined, 3].flat()));
show("null kept", JSON.stringify([1, null, [null]].flat()));
show("mixed", JSON.stringify([1, "x", , [true, , null], , undefined].flat()));

// Non-array elements (plain objects) are pushed as-is, not flattened.
show("object elem", JSON.stringify([{ a: 1 }].flat()));
show("object + array", JSON.stringify([{ a: 1 }, , [{ b: 2 }]].flat()));

// Regression: arrays with no holes flatten normally.
show("no holes d1", JSON.stringify([1, [2, 3], [4, [5]]].flat()));
show("no holes d2", JSON.stringify([1, [2, [3]]].flat(2)));
show("deep no hole", JSON.stringify([1, [2, [3, [4]]]].flat(Infinity)));
show("empty", JSON.stringify([].flat()));
