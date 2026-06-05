// Object.keys / Object.values / Object.entries and the `in` operator on
// arrays. `Object.values`/`Object.entries` previously had no array branch and
// crashed (the ArrayHeader was read as an ObjectHeader). They must enumerate
// present (non-hole) elements as string-keyed own properties, then enumerable
// named properties. After `arr.length = N` grows (and relocates) the array,
// a binding still holds the old forwarding pointer, so the enumeration must
// resolve the forwarding chain (issue #233).

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

// values / entries on normal arrays (these used to crash).
show("values", JSON.stringify(Object.values([10, 20, 30])));
show("entries", JSON.stringify(Object.entries([10, 20])));
show("values empty", JSON.stringify(Object.values([])));
show("values str", JSON.stringify(Object.values(["a", "b"])));
show("entries mixed", JSON.stringify(Object.entries([1, "x", true])));

// Holes are skipped.
show("values holes", JSON.stringify(Object.values([1, , 3])));
show("entries holes", JSON.stringify(Object.entries([1, , 3])));
show("keys holes", JSON.stringify(Object.keys([1, , 3])));

// After a length grow (forwarding pointer resolution).
const a = [1, 2]; a.length = 4;
show("keys grow", JSON.stringify(Object.keys(a)));
const b = [1, 2]; b.length = 4;
show("values grow", JSON.stringify(Object.values(b)));
const c = [1, 2]; c.length = 4;
show("entries grow", JSON.stringify(Object.entries(c)));
const d = [1, 2]; d.length = 4;
show("in grow", (1 in d) + "," + (3 in d));
const e = [1, 2]; e.length = 4;
let fk = "";
for (const k in e) fk += k + ",";
show("forin grow", fk);
const g = [1, 2]; g.length = 100;
show("big grow values", JSON.stringify(Object.values(g)));

// Enumerable named properties on an array are included after the indices.
const h: any = [1, 2]; h.foo = "bar";
show("named keys", JSON.stringify(Object.keys(h)));
show("named values", JSON.stringify(Object.values(h)));
show("named entries", JSON.stringify(Object.entries(h)));

// Iteration on a grown array still works.
const m = [1, 2]; m.length = 4;
show("map grow", JSON.stringify(m.map((x) => x)));
