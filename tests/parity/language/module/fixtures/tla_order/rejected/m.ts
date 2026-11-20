console.log("m1");
await Promise.reject(new Error("boom"));
console.log("unexpected m2");
export {};
