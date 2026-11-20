const h = new Headers();
h.append("b", "2");
h.append("a", "1");

console.log("iterator typeof:", typeof (h as any)[Symbol.iterator]);
console.log("entries typeof:", typeof h.entries);
console.log("iterator equals entries:", (h as any)[Symbol.iterator] === h.entries);

const viaForOf: string[] = [];
for (const [key, value] of h as any) {
  viaForOf.push(`${key}=${value}`);
}
console.log("for-of:", viaForOf.join(","));

console.log("spread:", JSON.stringify([...(h as any)]));
console.log("array from:", JSON.stringify(Array.from(h as any)));
console.log("entries:", JSON.stringify(Array.from(h.entries())));
