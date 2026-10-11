// #12243: callable values follow the live Function.prototype chain when
// checking membership in Object.
const ordinary = function () {};
const arrow = () => 1;
const functionPrototypeParent = Object.getPrototypeOf(Function.prototype);

console.log("initial:", ordinary instanceof Object, arrow instanceof Object);
Object.setPrototypeOf(Function.prototype, null);
console.log("after null:", ordinary instanceof Object, arrow instanceof Object);
Object.setPrototypeOf(Function.prototype, functionPrototypeParent);
console.log("restored:", ordinary instanceof Object, arrow instanceof Object);
