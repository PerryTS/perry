// A spread among the arguments expands before the call.
const k = Number(process.argv[2] ?? "0");
const one = [k];
const two = [k + 1, 2];
const three = [k + 3, 4, 5];
const none: number[] = [];
console.log(Math.cos(...one), Math.cos(...two), Math.cos(...none));
console.log(Math.atan2(...two), Math.atan2(...one), Math.pow(...two), Math.pow(...three));
console.log(Math.max(...three), Math.min(...three), Math.max(...none), Math.min(...none), Math.hypot(...three), Math.hypot(...none));
console.log(Math.max(k, ...three), Math.min(...three, k), Math.hypot(k, ...two));
console.log(Math.cos(k, ...two), Math.atan2(k, ...two), Math.imul(...three));
