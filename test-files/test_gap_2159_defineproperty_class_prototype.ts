// #2159 — Object.defineProperty(Class.prototype, name, descriptor) and the
// `applyMixins(Target, [Source])` pattern that copies methods between class
// prototypes (drizzle-orm's `MySqlSelectBase` borrows `then`/`catch`/`finally`
// from `QueryPromise` this way).
//
// Pre-fix, `Class.prototype` evaluated to the class-ref itself in Perry
// (not a separate prototype object), so `Object.defineProperty(C.prototype,
// name, desc)` hit `extract_obj_ptr → null` in the runtime and silently
// dropped the descriptor. Instance lookups (`(new C()).method`) never saw
// the new property, so `await db.select().from(x)` returned the builder
// itself instead of resolving the query.

// 1. Single-method install via a plain function value.
class A { x() { return "x-original"; } }
Object.defineProperty(
  (A as any).prototype,
  "y",
  { value: function (this: any) { return "y-installed"; }, writable: true, configurable: true },
);
const a = new A();
console.log("typeof a.x:", typeof a.x);
console.log("typeof a.y:", typeof (a as any).y);
console.log("a.x():", a.x());
console.log("a.y():", (a as any).y());

// 2. Mixin pattern: copy descriptors from one class's prototype onto another.
//    Receiver-binding check: the copied method's `this` is the receiver, so
//    `this.execute()` finds the target class's own `execute`.
class QueryPromise {
  catch(this: any, onR?: any) { return this.then(undefined, onR); }
  then(this: any, onF?: any, onR?: any) { return this.execute().then(onF, onR); }
}
class Builder {
  execute = async () => ["row1", "row2", "row3"];
}
function applyMixins(baseClass: any, extendedClasses: any[]) {
  for (const ext of extendedClasses) {
    for (const name of Object.getOwnPropertyNames(ext.prototype)) {
      if (name === "constructor") continue;
      Object.defineProperty(
        baseClass.prototype,
        name,
        Object.getOwnPropertyDescriptor(ext.prototype, name)!,
      );
    }
  }
}
applyMixins(Builder, [QueryPromise]);

const b = new Builder();
console.log("typeof b.then:", typeof (b as any).then);
console.log("typeof b.catch:", typeof (b as any).catch);

// 3. `await <thenable>` assimilation — the inherited `then` must be
//    callable so the await loop sees a real thenable and routes through
//    `js_assimilate_thenable`.
const rows = await (b as any);
console.log("await b len:", rows.length);
console.log("await b[0]:", rows[0]);
