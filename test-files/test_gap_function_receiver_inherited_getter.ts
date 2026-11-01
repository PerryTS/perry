// A getter inherited two or more prototype levels up runs with the function
// the property was read from as `this`, not with the prototype the lookup
// entered through.
class Base {
  get self() {
    return this;
  }
  get kind() {
    return typeof this;
  }
}
class Mid extends Base {}
class Leaf extends Mid {}

const fn = Object.setPrototypeOf(function () {}, Mid.prototype) as any;
console.log(fn.self === fn, fn.kind);

const bound = Object.setPrototypeOf(function () {}.bind(null), Mid.prototype) as any;
console.log(bound.self === bound, bound.kind);

const deep = Object.setPrototypeOf(() => 1, Leaf.prototype) as any;
console.log(deep.self === deep, deep.kind);

const obj = Object.setPrototypeOf({}, Mid.prototype) as any;
console.log(obj.self === obj, obj.kind);

// instanceof follows the prototype a function object was given.
console.log(fn instanceof Mid, fn instanceof Base, fn instanceof Leaf, fn instanceof Function);
console.log(bound instanceof Mid, deep instanceof Leaf, deep instanceof Base, deep instanceof Function);
console.log(obj instanceof Mid, obj instanceof Leaf);
