// Upstream WHATWG URLSearchParams coverage has a lot of duplicate-key cases.
// Keep a compact parity probe that checks order-preserving append, getAll,
// value-specific has/delete and set() duplicate collapse.
const sp = new URLSearchParams("a=1&a=2&b=3");
console.log("initial:", sp.toString());
console.log("getAll a:", sp.getAll("a").join("|"));
console.log("has a=2:", sp.has("a", "2"));
console.log("has a=4:", sp.has("a", "4"));
sp.append("a", "4");
console.log("after append:", sp.toString());
sp.delete("a", "2");
console.log("after delete value:", sp.toString());
sp.set("a", "9");
console.log("after set:", sp.toString());
