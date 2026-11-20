// A constant initialized before any user code runs is available to every
// later function call and remains eligible for module-constant folding.
const K = 3;
function afterInit(value: number): number { return value * K; }
console.log("after:", afterInit(14));

// Function declarations are hoisted, but their bodies run only when called.
// This call precedes `later` and therefore must retain the TDZ check.
function beforeInit(): number { return later; }
try {
  console.log(beforeInit());
} catch (error: any) {
  console.log(error.constructor.name);
}
const later = 9;
console.log("later:", beforeInit());
