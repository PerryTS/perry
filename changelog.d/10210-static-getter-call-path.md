Fix method calls inside static getters of classes that extend a call expression
(effect v4 `class Svc extends Context.Service<Svc, Shape>()(id) {}` and
OpenCode's `ConfigService.Service` wrapper). `this.of(x)` in such a getter threw
"of is not a function" although reading `this.of` returned the function: the
dynamic method-call dispatcher resolved only the static-method vtable for a
per-evaluation class object, and the class-ref read path looked for the
function-valued parent only on the receiver's own class. The class-object call
arm now resolves the name exactly as the read path does (static accessors, the
parent class object's statics, the ancestor function's swapped prototype) and
calls the closure with `this` bound to the receiver; string- and symbol-keyed
static reads walk the class chain for the function-valued ancestor. This
unblocks OpenCode v1.18.30's runtime bootstrap (#10210).
