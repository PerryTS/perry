// Issue #36 / #321 (effect Context/Layer): a class DECLARATION that `extends`
// a plain FUNCTION value (not a class) must inherit the parent function's OWN
// static properties AND any properties on the function's static prototype
// (`Object.setPrototypeOf(fn, protoObj)`).
//
// effect's `Context.Tag(id)` returns a function `TagClass` with `TagClass.key
// = id` (an OWN static) whose `_op: "Tag"` / `[TagTypeId]` live on a `TagProto`
// object wired in via `Object.setPrototypeOf(TagClass, TagProto)`. User code
// then writes `class Svc extends Context.Tag("Svc")<...>() {}`, so reading
// `Svc.key` / `Svc._op` must walk the static prototype chain to the parent
// function and ITS proto. Pre-fix Perry returned `undefined` for all of them,
// which made effect treat the Tag as a non-Effect and the fiber died with a
// `Cause.die(<TypeError reading '_V'>)` that pretty-printed as `{}`.
//
// Expected output:
// Parent.key direct: K
// Parent._op direct: Tag
// Child.key: K
// Child._op: Tag
// Child.marker: 123
// typeof Child: function

const Proto: any = { _op: "Tag", marker: 123 };
function makeParent(id: string) {
  function P() {}
  Object.setPrototypeOf(P, Proto);
  (P as any).key = id;
  return P as any;
}

const Parent = makeParent("K");
console.log("Parent.key direct:", (Parent as any).key);
console.log("Parent._op direct:", (Parent as any)._op);

class Child extends Parent {}
console.log("Child.key:", (Child as any).key);
console.log("Child._op:", (Child as any)._op);
console.log("Child.marker:", (Child as any).marker);
console.log("typeof Child:", typeof Child);
