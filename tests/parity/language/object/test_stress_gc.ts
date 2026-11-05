// Stress test: Garbage Collection correctness
// Targets: GC root scanning, arena+malloc interaction, large-alloc triggers
// Based on bugs from v0.5.25 (#34), v0.5.26-27 (#35), v0.5.28 (#36),
// v0.5.33 (#43/#44), v0.5.37 (#46)

// === SECTION: Objects survive GC in allocation loop ===
// Create many objects, verify they're all intact after allocation pressure
const objects: any[] = [];
for (let i = 0; i < 5000; i++) {
  objects.push({ id: i, name: "item_" + i, value: i * 3.14 });
}
// Verify first, middle, last are intact
console.log("obj[0].id:", objects[0].id);
console.log("obj[0].name:", objects[0].name);
console.log("obj[2500].id:", objects[2500].id);
console.log("obj[4999].id:", objects[4999].id);
console.log("obj count:", objects.length);

// === SECTION: Strings survive allocation pressure ===
const strings: string[] = [];
for (let i = 0; i < 3000; i++) {
  strings.push("string_number_" + i + "_with_extra_content");
}
console.log("str[0]:", strings[0]);
console.log("str[1500]:", strings[1500]);
console.log("str[2999]:", strings[2999]);
console.log("str count:", strings.length);

// === SECTION: Nested objects and arrays survive GC ===
const nested: any[] = [];
for (let i = 0; i < 1000; i++) {
  nested.push({
    arr: [i, i + 1, i + 2],
    inner: { x: i * 2, y: "val_" + i },
  });
}
console.log("nested[0].arr:", nested[0].arr);
console.log("nested[0].inner.x:", nested[0].inner.x);
console.log("nested[0].inner.y:", nested[0].inner.y);
console.log("nested[500].arr[2]:", nested[500].arr[2]);
console.log("nested[999].inner.y:", nested[999].inner.y);

// === SECTION: Map survives allocation pressure ===
const map = new Map<string, number>();
for (let i = 0; i < 2000; i++) {
  map.set("key_" + i, i * 7);
}
// Allocate more stuff to trigger potential GC
const filler: number[] = [];
for (let i = 0; i < 10000; i++) {
  filler.push(i);
}
// Verify map is still intact
console.log("map.size:", map.size);
console.log("map.get(key_0):", map.get("key_0"));
console.log("map.get(key_999):", map.get("key_999"));
console.log("map.get(key_1999):", map.get("key_1999"));

// === SECTION: JSON parse large array (>1666 records trigger) ===
// v0.5.37 bug: GC could sweep in-progress parse_array frames
const items: any[] = [];
for (let i = 0; i < 2000; i++) {
  items.push({ id: i, val: "item" + i });
}
const jsonStr = JSON.stringify(items);
const parsed = JSON.parse(jsonStr);
console.log("parsed.length:", parsed.length);
console.log("parsed[0].id:", parsed[0].id);
console.log("parsed[0].val:", parsed[0].val);
console.log("parsed[1000].id:", parsed[1000].id);
console.log("parsed[1999].val:", parsed[1999].val);

// === SECTION: Closures survive allocation pressure ===
// v0.5.26/27 bug: closures in event listeners could be swept
const closures: (() => string)[] = [];
for (let i = 0; i < 1000; i++) {
  const captured = "closure_" + i;
  closures.push(() => captured);
}
// Allocate more to potentially trigger GC
for (let i = 0; i < 5000; i++) {
  const temp = { x: i };
}
// Verify closures still work
console.log("closure[0]():", closures[0]());
console.log("closure[500]():", closures[500]());
console.log("closure[999]():", closures[999]());

// === SECTION: Mixed types in arrays survive GC ===
const mixed: any[] = [];
for (let i = 0; i < 1000; i++) {
  mixed.push(i);                          // number
  mixed.push("str_" + i);                 // string
  mixed.push(i % 2 === 0);               // boolean
  mixed.push([i, i + 1]);                 // array
  mixed.push({ n: i });                   // object
}
console.log("mixed.length:", mixed.length);
console.log("mixed[0]:", mixed[0]);
console.log("mixed[1]:", mixed[1]);
console.log("mixed[2]:", mixed[2]);
console.log("mixed[3]:", mixed[3]);
console.log("mixed[4].n:", mixed[4].n);
// Check near the end
console.log("mixed[4995]:", mixed[4995]);
console.log("mixed[4996]:", mixed[4996]);
console.log("mixed[4997]:", mixed[4997]);

// === SECTION: Set survives allocation pressure ===
const set = new Set<string>();
for (let i = 0; i < 2000; i++) {
  set.add("set_item_" + i);
}
const moreObjects: any[] = [];
for (let i = 0; i < 5000; i++) {
  moreObjects.push({ filler: i });
}
console.log("set.size:", set.size);
console.log("set.has(set_item_0):", set.has("set_item_0"));
console.log("set.has(set_item_999):", set.has("set_item_999"));
console.log("set.has(set_item_1999):", set.has("set_item_1999"));
console.log("set.has(nonexistent):", set.has("nonexistent"));

// === SECTION: Deep JSON roundtrip under pressure ===
const deepObj = {
  a: { b: { c: { d: { e: "deep" } } } },
  arr: [[1, 2], [3, 4], [5, 6]],
  mixed: [1, "two", true, null, { x: 99 }],
};
for (let i = 0; i < 100; i++) {
  const str = JSON.stringify(deepObj);
  const re = JSON.parse(str);
  if (i === 99) {
    console.log("roundtrip.a.b.c.d.e:", re.a.b.c.d.e);
    console.log("roundtrip.arr[2][1]:", re.arr[2][1]);
    console.log("roundtrip.mixed[4].x:", re.mixed[4].x);
  }
}
