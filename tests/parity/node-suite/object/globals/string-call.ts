const values = ["a", 2, true, undefined, null];

console.log("typeof:", typeof String);
console.log("direct:", String("x"), String(2), String(true), String(undefined), String(null));
console.log("mapped:", values.map(String).join("|"));

const awaited = await Promise.resolve(values);
console.log("await joined:", awaited.join("|"));
console.log("await mapped:", awaited.map(String).join("|"));
