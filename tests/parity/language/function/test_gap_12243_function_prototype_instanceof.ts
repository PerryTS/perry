// #12243: callable values follow the live Function.prototype chain when
// checking membership in Object.
const ordinary = function () {};
const arrow = () => 1;
// DEC 250 locks Function.prototype's [[Prototype]]; the functions adopt a
// user function prototype FP (below Function.prototype) whose link changes.
const FP = Object.create(Function.prototype);
Object.setPrototypeOf(ordinary, FP);
Object.setPrototypeOf(arrow, FP);
const functionPrototypeParent = Object.getPrototypeOf(FP);

console.log("initial:", ordinary instanceof Object, arrow instanceof Object);
Object.setPrototypeOf(FP, null);
console.log("after null:", ordinary instanceof Object, arrow instanceof Object);
Object.setPrototypeOf(FP, functionPrototypeParent);
console.log("restored:", ordinary instanceof Object, arrow instanceof Object);
