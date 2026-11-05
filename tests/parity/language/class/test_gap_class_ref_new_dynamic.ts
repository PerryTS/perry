// A class held as a first-class value (aliased through a local variable or
// an object field), then constructed via `new <value>()`, must produce an
// instance whose inherited prototype methods dispatch.
//
// Perry lowers a bare `class` identifier used as a *value* to an INT32-tagged
// class-ref (`INT32_TAG | class_id`). When that value reaches the dynamic
// `new` helper (`js_new_function_construct`) via a `LocalGet` / `PropertyGet`
// callee, it was neither a heap class-object nor a pointer-shaped closure, so
// the synthetic-class-id lookup returned 0 and the instance was stamped with
// class_id 0 — losing every inherited prototype method
// (`<m> is not a function`). This is the same class-as-value family that
// blocks effect's Layer/Scope machinery (#321), where a `Context.Tag`
// subclass is passed through generic combinators.
class Animal {
  kind(): string {
    return "animal";
  }
}
class Dog extends Animal {
  static species = "canine";
  bark(): string {
    return "woof";
  }
}

// Alias the class through a plain local, then construct.
const C: any = Dog;
const d = new C();
console.log(d.bark());
console.log(d.kind());
console.log(d instanceof Dog);
console.log(d instanceof Animal);

// Alias through an object field, then construct.
const holder: any = { ctor: Dog };
const d2 = new holder.ctor();
console.log(d2.bark());
console.log(d2.kind());

// Static read still works on the aliased reference.
console.log(C.species);
