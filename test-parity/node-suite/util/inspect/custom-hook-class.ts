import { inspect } from "node:util";

// #1248: `[util.inspect.custom]` declared on a class prototype is invoked
// when the formatter encounters an instance. Pre-fix the hook resolution
// only consulted the per-instance symbol side table — class methods route
// through prototype/class-static dispatch and never registered an entry
// for the instance.
class Foo {
  [inspect.custom]() {
    return "Foo<custom>";
  }
}
console.log(new Foo());

// Non-string returns recurse with the regular depth cap.
class Bar {
  [inspect.custom]() {
    return { kind: "bar" };
  }
}
console.log(new Bar());

// Inheritance: a hook on the base class fires for a subclass instance.
class Base {
  [inspect.custom]() {
    return "Base<custom>";
  }
}
class Sub extends Base {}
console.log(new Sub());

// #1249: nested multi-line objects force the outer to break too, and the
// nested body's continuation lines are re-indented to sit under the outer.
console.log({
  outer: {
    aaaa: 1,
    bbbb: 2,
    cccc: 3,
    dddd: 4,
    eeee: 5,
    ffff: 6,
    gggg: 7,
    hhhh: 8,
  },
});
