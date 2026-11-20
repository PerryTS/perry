console.time([]); console.timeEnd([]);
console.time({}); console.timeEnd({});
console.time(null); console.timeEnd(null);
console.time(undefined); console.timeEnd("default");
console.time(NaN); console.timeEnd(NaN);
console.time("__proto__"); console.timeEnd("__proto__");
console.time("constructor"); console.timeEnd("constructor");
