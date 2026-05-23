// Global timer functions are callable and report typeof "function".
console.log("setTimeout:", typeof setTimeout);
console.log("setInterval:", typeof setInterval);
console.log("setImmediate:", typeof setImmediate);
console.log("clearTimeout:", typeof clearTimeout);
console.log("clearInterval:", typeof clearInterval);
console.log("clearImmediate:", typeof clearImmediate);
let fired = 0;
const im = setImmediate(() => { fired++; });
clearImmediate(im);
await new Promise<void>((r) => setTimeout(() => r(), 15));
console.log("cleared immediate:", fired === 0);
