// Test Map collection type
let m: Map<string, number> = new Map();

// Test set and get
m.set("one", 1);
m.set("two", 2);
m.set("three", 3);

console.log(m.get("one"));    // 1
console.log(m.get("two"));    // 2
console.log(m.get("three"));  // 3

// Test has
console.log(m.has("two"));    // 1 (true)
console.log(m.has("four"));   // 0 (false)

// Test size
console.log(m.size);          // 3

// Test delete
m.delete("two");
console.log(m.has("two"));    // 0 (false)
console.log(m.size);          // 2

// Test update existing key
m.set("one", 10);
console.log(m.get("one"));    // 10

// Test clear
m.clear();
console.log(m.size);          // 0

console.log("Map tests passed!");
