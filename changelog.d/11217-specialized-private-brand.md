Fix private fields, methods, and accessors on explicitly specialized generic
class instances (#11183). `new Box<number>()` now installs private elements
under the original class declaration's identity, matching the guards and
storage keys retained in specialized method bodies.

Resolve specialization origins transitively during field initialization while
keeping inheritance separate, so base and derived private names remain distinct.
Add an emitted-IR regression for ordinary, specialized, transitively specialized,
and unrelated classes, plus a native parity fixture covering private access,
brand tests, borrowed methods, inheritance, constructor writes, and rejection of
unrelated and prototype-only receivers.
