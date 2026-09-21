fix(hir): a `static get`/`static set` reads its own receiver's captures instead of the declaration site's (#10835).

A static accessor on a class that closes over its enclosing function's arguments read the capture from the class's DECLARATION-site slot. Those slots are keyed by class name, so the last evaluation of the declaration wins, and every earlier class produced by the same factory silently answered with the last one's captured values.

Static *methods* already resolved per-receiver; only accessors took the decl-site path. This routes them to the same `ClassCaptureValue` strategy — the static accessors are filtered out of the instance-rewrite loops and get a per-capture prologue of their own, mirroring the static-method block.

This is Effect's `Context.Service` shape, which is why OpenCode's services collapsed onto a single tag:

```ts
function makeService(id: string, fields: Record<string, any>) {
  class ConfigTag extends serviceBase(id) {
    static fields = fields
    static get layer() { return { for: id } }
  }
  return ConfigTag
}
class Auth  extends makeService("tag-Auth",  { password: 1 }) {}
class Flags extends makeService("tag-Flags", { autoShare: 2 }) {}
```

`Auth.layer` answered `{"for":"tag-Flags"}` while `Auth.key` — a plain static field, a different path — stayed correct. That asymmetry is what makes it easy to miss, so the regression test pins both.
