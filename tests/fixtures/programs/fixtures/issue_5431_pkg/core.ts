// Issue #5431 — a `$`-prefixed exported function. The body is emitted
// under the INJECTIVE `sanitize_member` symbol (`perry_fn_<mod>__u__24constructor`)
// while cross-module callers resolve the plain-`sanitize` symbol
// (`perry_fn_<mod>___constructor`). Pre-fix those diverged: the #461 stub
// loop claimed the plain symbol with an undefined-returning body, so every
// cross-module CALL returned `undefined` (the reference itself was fine).
// This is the exact shape of zod v4's `core.$constructor`.
export function $constructor(name: string, initializer: (inst: any) => void): any {
  function init(inst: any) {
    inst._zod = inst._zod ?? {};
    inst._zod.traits ??= new Set();
    inst._zod.traits.add(name);
    initializer(inst);
    inst._zod.constr = _;
  }
  function _(this: any, def: any) {
    const inst = this;
    init(inst);
    inst._zod.def = def;
    return inst;
  }
  Object.defineProperty(_, "init", { value: init });
  Object.defineProperty(_, "name", { value: name });
  return _ as any;
}

// `$`-prefixed function returning a plain value (not a constructor).
export function $tag(label: string): string {
  return "tag:" + label;
}
