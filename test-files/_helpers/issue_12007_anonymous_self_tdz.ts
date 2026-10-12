// Importing this module must reject with ReferenceError during class
// evaluation because the anonymous expression's initializer reads its outer
// binding before that binding is initialized.
console.log("anonymous-evaluating");
const Anonymous = class {
  static self = Anonymous;
};
export { Anonymous };
