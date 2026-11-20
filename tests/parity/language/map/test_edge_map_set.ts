// Edge-case tests for Map and Set built-in collections
// Tests: basic operations, iteration, complex keys/values, size tracking

// --- Basic Map operations ---
const map = new Map<string, number>();
map.set("a", 1);
map.set("b", 2);
map.set("c", 3);

console.log(map.get("a"));  // 1
console.log(map.get("b"));  // 2
console.log(map.get("c"));  // 3
console.log(map.size);       // 3

// --- Map.has ---
console.log(map.has("a"));  // true
console.log(map.has("z"));  // false

// --- Map.delete ---
map.delete("b");
console.log(map.has("b"));  // false
console.log(map.size);       // 2

// --- Map overwrite ---
const m2 = new Map<string, number>();
m2.set("x", 10);
m2.set("x", 20);
console.log(m2.get("x"));  // 20
console.log(m2.size);       // 1

// --- Map with number keys ---
const numMap = new Map<number, string>();
numMap.set(1, "one");
numMap.set(2, "two");
numMap.set(3, "three");
console.log(numMap.get(1));    // one
console.log(numMap.get(2));    // two
console.log(numMap.size);       // 3

// --- Map iteration with forEach ---
const forEachResult: string[] = [];
const m3 = new Map<string, number>();
m3.set("x", 10);
m3.set("y", 20);
m3.set("z", 30);
m3.forEach((value: number, key: string) => {
    forEachResult.push(key + "=" + value.toString());
});
console.log(forEachResult.join(","));  // x=10,y=20,z=30

// --- Map.keys ---
const keyMap = new Map<string, number>();
keyMap.set("a", 1);
keyMap.set("b", 2);
keyMap.set("c", 3);
const mapKeys = Array.from(keyMap.keys());
console.log(mapKeys.join(","));  // a,b,c

// --- Map.values ---
const mapVals = Array.from(keyMap.values());
let mapValSum = 0;
for (let i = 0; i < mapVals.length; i++) {
    mapValSum = mapValSum + mapVals[i];
}
console.log(mapValSum);  // 6

// --- Map.clear ---
const clearMap = new Map<string, number>();
clearMap.set("a", 1);
clearMap.set("b", 2);
clearMap.clear();
console.log(clearMap.size);  // 0

// --- Map from entries ---
const fromEntries = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
console.log(fromEntries.get("a"));  // 1
console.log(fromEntries.get("c"));  // 3
console.log(fromEntries.size);       // 3

// === Set Tests ===

// --- Basic Set operations ---
const set = new Set<number>();
set.add(1);
set.add(2);
set.add(3);
console.log(set.size);      // 3
console.log(set.has(1));     // true
console.log(set.has(99));    // false

// --- Set deduplication ---
const dedup = new Set<number>();
dedup.add(1);
dedup.add(2);
dedup.add(1);
dedup.add(3);
dedup.add(2);
console.log(dedup.size);    // 3

// --- Set.delete ---
const delSet = new Set<number>();
delSet.add(10);
delSet.add(20);
delSet.add(30);
delSet.delete(20);
console.log(delSet.has(20));  // false
console.log(delSet.size);     // 2

// --- Set with strings ---
const strSet = new Set<string>();
strSet.add("hello");
strSet.add("world");
strSet.add("hello");  // duplicate
console.log(strSet.size);         // 2
console.log(strSet.has("hello")); // true
console.log(strSet.has("foo"));   // false

// --- Set iteration with forEach ---
const setItems: number[] = [];
const iterSet = new Set<number>();
iterSet.add(10);
iterSet.add(20);
iterSet.add(30);
iterSet.forEach((value: number) => {
    setItems.push(value);
});
console.log(setItems.join(","));  // 10,20,30

// --- Set.clear ---
const clearSet = new Set<number>();
clearSet.add(1);
clearSet.add(2);
clearSet.clear();
console.log(clearSet.size);  // 0

// --- Set from array ---
const fromArr = new Set([1, 2, 3, 2, 1]);
console.log(fromArr.size);  // 3

// --- Array deduplication pattern using Set ---
const dupes = [1, 2, 3, 2, 4, 3, 5, 1];
const unique = Array.from(new Set(dupes));
console.log(unique.length);      // 5
console.log(unique.join(","));   // 1,2,3,4,5

// --- Set intersection pattern ---
const setA = new Set([1, 2, 3, 4, 5]);
const setB = new Set([3, 4, 5, 6, 7]);
const intersection: number[] = [];
setA.forEach((val: number) => {
    if (setB.has(val)) {
        intersection.push(val);
    }
});
console.log(intersection.join(","));  // 3,4,5

// --- Set union pattern ---
const union = new Set<number>();
setA.forEach((val: number) => union.add(val));
setB.forEach((val: number) => union.add(val));
console.log(union.size);  // 7

// --- Map with complex values ---
const complexMap = new Map<string, number[]>();
complexMap.set("evens", [2, 4, 6]);
complexMap.set("odds", [1, 3, 5]);
const evens = complexMap.get("evens");
if (evens) {
    console.log(evens.join(","));  // 2,4,6
}

// --- Map as counter ---
function countChars(s: string): Map<string, number> {
    const counts = new Map<string, number>();
    for (let i = 0; i < s.length; i++) {
        const ch = s[i];
        const current = counts.get(ch);
        counts.set(ch, (current !== undefined ? current : 0) + 1);
    }
    return counts;
}

const charCounts = countChars("abracadabra");
console.log(charCounts.get("a"));  // 5
console.log(charCounts.get("b"));  // 2
console.log(charCounts.get("r"));  // 2

// --- Map/Set size after many operations ---
const stressMap = new Map<number, number>();
for (let i = 0; i < 100; i++) {
    stressMap.set(i, i * i);
}
console.log(stressMap.size);         // 100
console.log(stressMap.get(50));      // 2500
console.log(stressMap.get(99));      // 9801

for (let i = 0; i < 50; i++) {
    stressMap.delete(i);
}
console.log(stressMap.size);         // 50
console.log(stressMap.has(0));       // false
console.log(stressMap.has(50));      // true
