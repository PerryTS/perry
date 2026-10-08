// A fresh class's constructor inherits symbols from its evaluated parent,
// including when another declared class extends that fresh class (#12191).
function parent(label: string) {
  function Base() {}
  Object.setPrototypeOf(Base, {
    [Symbol.iterator]() {
      const receiver = this;
      let done = false;
      return {
        next() {
          const result = { value: receiver, done };
          done = true;
          return result;
        },
      };
    },
    label,
  });
  return Base;
}
function middle(label: string) {
  class Middle extends parent(label) {}
  return Middle;
}
class First extends middle("first") {}
class Second extends middle("second") {}
function* visit(value: any) { yield* value; }
console.log(visit(First).next().value === First);
console.log(visit(Second).next().value === Second);
console.log((First as any).label, (Second as any).label);
