const base = { marker: 1 };

console.log("Object() typeof:", typeof Object());
console.log("Object(null) typeof:", typeof Object(null));
console.log("Object(undefined) typeof:", typeof Object(undefined));
console.log("Object(base) same:", Object(base) === base);
console.log("new Object(null) typeof:", typeof new Object(null));
