const h = new Headers();
h.set("b", "2");
h.set("a", "1");

console.log("iterator type:", typeof h[Symbol.iterator]);
console.log("entries type:", typeof h.entries);
console.log("same function:", h[Symbol.iterator] === h.entries);
console.log("spread:", JSON.stringify([...h]));
console.log("array from:", JSON.stringify(Array.from(h)));
console.log("direct call:", JSON.stringify(Array.from(h[Symbol.iterator]())));

const seen: string[] = [];
for (const [key, value] of h) {
  seen.push(key + "=" + value);
}
console.log("for-of:", JSON.stringify(seen));
