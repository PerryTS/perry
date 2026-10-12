// Standalone subprocess witness for the direct anonymous class-expression
// initializer read. Evaluation must terminate with a ReferenceError because
// the outer binding remains uninitialized until class evaluation completes.
console.log("anonymous-evaluating");
const Anonymous = class {
  static self = Anonymous;
};
export { Anonymous };
