export let [value] = [7];
console.log("m1");
await Promise.resolve();
value = 9;
console.log("m2");
