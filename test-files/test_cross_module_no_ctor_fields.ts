// Refs v0.5.758: cross-module class with extends + field initializers but
// NO own constructor. Pre-fix the leaf class's field initializers
// (`config = {x:1}`, `arrow = () => ...`) were lost end-to-end:
//   - lower_new walked past the leaf's synthesized ctor symbol to find
//     the closest ancestor with `param_count > 0`, calling THAT ctor
//     directly. The leaf's synthesized ctor — which would have applied
//     SelfOnly inits — was never invoked.
//   - lower_new's post-init re-apply at the call site uses the STUB's
//     fields (`init: None` for every imported field), writing Undefined
//     over whatever the chain just set.
// Two coordinated fixes: walk-stop at the first class with ANY synthesized
// imported_class_ctor symbol, and skip the post-init re-apply when the
// imported ctor is invoked.
import { Child } from "./_helpers/cross_module_no_ctor_child.ts";

const c: any = new Child();
console.log("typeof c:", typeof c);
console.log("typeof c.config:", typeof c.config);
console.log("c.config.x:", c.config?.x);
console.log("typeof c.arrow:", typeof c.arrow);
console.log("c.arrow():", c.arrow());
console.log("c.parentField:", (c as any).parentField);
