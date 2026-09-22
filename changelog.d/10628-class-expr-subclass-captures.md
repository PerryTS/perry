### Fixed

- **An inherited method of a capture-bearing class expression could read
  `undefined` captures when called on a subclass instance.** A subclass
  with no explicit constructor, extending a capture-bearing class
  EXPRESSION bound to a local (`const Base = class { m() { return cap; } };
  const Sub = class extends Base { n() {...} };`), never forwarded `Base`'s
  captured enclosing-scope locals to the synthesized subclass constructor —
  the lowering deliberately drops the static `extends_name` for such a
  lexically-local heritage identifier (avoiding a same-named-class
  collision, #5437), and capture propagation was keyed off that same name.
  Capture forwarding now resolves the heritage identifier through the same
  let/const class-alias table `new X()` construction already uses, scoped
  to subclasses that have their own member and no explicit constructor of
  their own (an explicit constructor already forwarded captures correctly
  through a separate mechanism). This was blocking `typescript`'s CJS
  `transpileModule` output (`IdentifierNameMultiMap extends
  IdentifierNameMap`, both class expressions with their own methods, in the
  bundled `typescript.js`). A subclass with a completely empty body, or
  whose base is a class DECLARATION rather than expression, hits a
  separate, still-open codegen field-layout gap (#10486).
