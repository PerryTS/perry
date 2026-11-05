// Step 1 test: typed parse must fall through gracefully when runtime
// JSON doesn't match the declared shape. No crash, semantics identical
// to untyped parse.

interface Item {
  id: number;
  name: string;
}

// Wrong top-level type (object, not array)
const obj = '{"id":1,"name":"a"}';
const r1 = JSON.parse<Item[]>(obj);
// Falls back to generic — result is the object, not an array
console.log("r1.id:" + (r1 as any).id);

// Wrong field types at runtime — keys match, values don't
const bad = '[{"id":"not_a_number","name":42}]';
const r2 = JSON.parse<Item[]>(bad);
console.log("r2.length:" + r2.length);
console.log("r2[0].id:" + r2[0].id);
console.log("r2[0].name:" + r2[0].name);
