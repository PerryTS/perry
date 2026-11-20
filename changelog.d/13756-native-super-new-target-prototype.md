`class C extends zlib.Gzip {}` instances get `C.prototype`: `super()` into a
native constructor export relinks the object it returns to new.target's
prototype, so `new C() instanceof C` is `true`, as in node.
