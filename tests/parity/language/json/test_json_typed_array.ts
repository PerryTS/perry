// Issue #179 Step 1b: JSON.parse<T[]> specialized path.
// Must match Node byte-for-byte despite the fast-path specialization.

interface Item {
  id: number;
  name: string;
  active: boolean;
}

const blob = '[{"id":1,"name":"alpha","active":true},{"id":2,"name":"beta","active":false},{"id":3,"name":"gamma","active":true}]';

const typed = JSON.parse<Item[]>(blob);
console.log("length:" + typed.length);
for (let i = 0; i < typed.length; i++) {
  console.log("[" + i + "] id=" + typed[i].id + " name=" + typed[i].name + " active=" + typed[i].active);
}

// Out-of-order fields must fall back gracefully
const oo = '[{"active":false,"name":"delta","id":4}]';
const typed2 = JSON.parse<Item[]>(oo);
console.log("oo.id:" + typed2[0].id);
console.log("oo.name:" + typed2[0].name);
console.log("oo.active:" + typed2[0].active);

// Extra fields (not in T) must still be present
const extra = '[{"id":5,"name":"epsilon","active":true,"bonus":42}]';
const typed3 = JSON.parse<Item[]>(extra);
console.log("extra.id:" + typed3[0].id);
console.log("extra.bonus:" + (typed3[0] as any).bonus);

// Untyped parse must give the same values (sanity check)
const untyped = JSON.parse(blob) as Item[];
console.log("untyped.length:" + untyped.length);
console.log("untyped[1].name:" + untyped[1].name);
