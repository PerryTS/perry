export let ready = false;
console.log("m1");
await new Promise(resolve => setTimeout(resolve, 10));
ready = true;
console.log("m2");
