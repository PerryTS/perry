// JSON.stringify must resolve a `toJSON()` method defined on a class PROTOTYPE
// (or an Object.create proto), not just an own property — both the no-replacer
// and replacer paths. Broadly useful for #321: effect's `Inspectable` defines
// `toJSON` on the prototype, so JSON of any class instance must honor it.

// --- class instance: toJSON on the prototype (not an own field) ---
class C {
  x = 1;
  toJSON() {
    return { tag: "C", x: this.x };
  }
}
console.log(JSON.stringify(new C())); // {"tag":"C","x":1}
console.log(JSON.stringify(new C(), (k, v) => v)); // {"tag":"C","x":1}

// --- Object.create(proto) inheriting toJSON from the prototype ---
const Proto: any = {
  toJSON() {
    return "PVAL";
  },
};
const o = Object.create(Proto);
(o as any).y = 9;
console.log(JSON.stringify(o)); // "PVAL"

// --- two-level Object.create chain ---
const Root: any = {
  toJSON() {
    return "ROOT";
  },
};
const Mid: any = Object.create(Root);
const leaf: any = Object.create(Mid);
leaf.z = 1;
console.log(JSON.stringify(leaf)); // "ROOT"

// --- class with no instance fields, only a prototype toJSON ---
class OnlyMethod {
  toJSON() {
    return 99;
  }
}
console.log(JSON.stringify(new OnlyMethod())); // 99

// --- prototype toJSON returning each scalar kind ---
class RetStr {
  z = 0;
  toJSON() {
    return "str";
  }
}
class RetBool {
  z = 0;
  toJSON() {
    return true;
  }
}
class RetNull {
  z = 0;
  toJSON() {
    return null;
  }
}
console.log(JSON.stringify(new RetStr())); // "str"
console.log(JSON.stringify(new RetBool())); // true
console.log(JSON.stringify(new RetNull())); // null

// --- inheritance: subclass inherits / overrides parent toJSON ---
class Base {
  v = 10;
  toJSON() {
    return { base: this.v };
  }
}
class Derived extends Base {}
class Overridden extends Base {
  toJSON() {
    return "D2";
  }
}
console.log(JSON.stringify(new Derived())); // {"base":10}
console.log(JSON.stringify(new Overridden())); // "D2"

// --- toJSON runs ONCE per value: a result that itself has a toJSON is
//     serialized as a plain object, NOT re-applied (ECMA-262 §25.5.2.2). ---
class Wrapper {
  toJSON() {
    return new C();
  }
}
console.log(JSON.stringify(new Wrapper())); // {"x":1}
console.log(JSON.stringify({ toJSON: () => new C() })); // {"x":1}

// --- nested / arrays of class instances ---
console.log(JSON.stringify({ inner: new C() })); // {"inner":{"tag":"C","x":1}}
console.log(JSON.stringify([new C(), new C()])); // [{"tag":"C","x":1},{"tag":"C","x":1}]

// --- pretty-print honors the prototype toJSON + indent threading ---
console.log(JSON.stringify({ p: new C() }, null, 2));
// {
//   "p": {
//     "tag": "C",
//     "x": 1
//   }
// }

// --- replacer + prototype toJSON + indent ---
console.log(JSON.stringify({ pt: new C() }, (k, v) => v, 2));

// --- NO regression: plain objects / own toJSON / primitives still work ---
console.log(JSON.stringify({})); // {}
console.log(JSON.stringify({ a: 1, b: "x", c: [1, 2, 3] })); // {"a":1,"b":"x","c":[1,2,3]}
console.log(JSON.stringify({ toJSON() { return 5; } })); // 5
console.log(JSON.stringify({ toJSON() { return { z: 9 }; } })); // {"z":9}
class NoToJSON {
  a = 1;
  b = 2;
}
console.log(JSON.stringify(new NoToJSON())); // {"a":1,"b":2}
const Plain: any = {
  greet() {
    return "hi";
  },
};
const op = Object.create(Plain);
(op as any).y = 9;
console.log(JSON.stringify(op)); // {"y":9}
console.log(JSON.stringify(42)); // 42
console.log(JSON.stringify("hi")); // "hi"
console.log(JSON.stringify(null)); // null
console.log(JSON.stringify([1, "a", true, null])); // [1,"a",true,null]

// --- built-in Date prototype toJSON still serializes as ISO ---
console.log(JSON.stringify(new Date(0))); // "1970-01-01T00:00:00.000Z"
console.log(JSON.stringify({ d: new Date(86400000) })); // {"d":"1970-01-02T00:00:00.000Z"}
