// Test Set collection type
let s: Set<string> = new Set();

// Test add
s.add("one");
s.add("two");
s.add("three");
s.add("two");  // duplicate - should not increase size

// Test size
console.log(s.size);  // 3

// Note: Set.has/delete/clear require lowering to differentiate from Map
// For now, just test add and size

console.log("Set tests passed!");
