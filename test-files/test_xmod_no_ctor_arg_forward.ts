// Refs v0.5.760: cross-module class without own ctor, where parent (in
// another module) has an explicit ctor — `new Child(arg)` now forwards
// `arg` to Parent_constructor. Pre-fix the synthesized Child ctor had
// ZERO params because the params-walking loop in compile_module looked
// up the parent in `class_table` (gets the STUB with constructor: None)
// and fell through without consulting `opts.imported_classes`'s
// `constructor_param_count`. Fix: consult imported_classes alongside
// the class_table lookup so the synthesized ctor adopts the same arity
// as the source-module parent ctor.
import { Child3 } from "./_helpers/xmod_arg_forward_child.ts";

const c: any = new Child3("PASS-ME-THROUGH");
console.log("config:", c.config);
console.log("extra:", c.extra);
