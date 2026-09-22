// #10589: `new X()` / `new (X as any)()` on an imported binding whose NAME
// collides with a Perry builtin constructor (Headers, EventEmitter, ...)
// constructed the BUILTIN instead of the user's own function/class of the
// same name. `peel_new_callee` strips a `(X as any)` cast before `new`'s
// lowering ever branches on the callee's shape, so the cast form and the
// bare-identifier form take the identical codegen path — the cast in the
// issue's title is not itself the trigger. The real gap: `lower_new_impl_inner`
// (crates/perry-codegen/src/lower_call/new.rs) called into the unconditional
// builtin-constructor table for any `class_name` absent from `ctx.classes`
// BEFORE checking whether that name is a user-imported PLAIN FUNCTION
// constructor (`ctx.import_function_prefixes`). Imported CLASSES already land
// in `ctx.classes` and always skipped the builtin table (see the `EventEmitter
// class` / `Stream class` controls below, which passed even before the fix) —
// only the function-declaration form was exposed.
//
// Each driver module below imports its constructor under the exact reserved
// name (`Headers`/`EventEmitter`/`Stream`) in its OWN module scope — a single
// module can only bind one top-level identifier per name, so each
// function-decl/class-decl x named/default-import combination needs its own
// tiny module. Every driver probes three shapes: `new X(...)` (plain),
// `new (X as any)(...)` (cast) and `new alias(...)` for a local variable
// holding the same value (the control that already worked — a regression
// there must be caught same as the other two).
//
// Note: this deliberately avoids `instanceof` as a discriminator (`#10477`,
// imported non-class constructors folding `x instanceof F` to `false`, is a
// separate bug not yet fixed on this base) — each library constructor stamps
// a `__mark: "user"` own-property instead, which a real Perry builtin
// Headers/EventEmitter never has.
import { run as headersFnNamed } from "./_helpers/new_cast_builtin_shadow_10589/d_headers_fn_named.ts";
import { run as headersFnDefault } from "./_helpers/new_cast_builtin_shadow_10589/d_headers_fn_default.ts";
import { run as headersClassNamed } from "./_helpers/new_cast_builtin_shadow_10589/d_headers_class_named.ts";
import { run as headersClassDefault } from "./_helpers/new_cast_builtin_shadow_10589/d_headers_class_default.ts";
import { run as emitterFnNamed } from "./_helpers/new_cast_builtin_shadow_10589/d_emitter_fn_named.ts";
import { run as emitterFnDefault } from "./_helpers/new_cast_builtin_shadow_10589/d_emitter_fn_default.ts";
import { run as emitterClassNamed } from "./_helpers/new_cast_builtin_shadow_10589/d_emitter_class_named.ts";
import { run as emitterClassDefault } from "./_helpers/new_cast_builtin_shadow_10589/d_emitter_class_default.ts";
import { run as streamFnNamed } from "./_helpers/new_cast_builtin_shadow_10589/d_stream_fn_named.ts";
import { run as streamFnDefault } from "./_helpers/new_cast_builtin_shadow_10589/d_stream_fn_default.ts";
import { run as streamClassNamed } from "./_helpers/new_cast_builtin_shadow_10589/d_stream_class_named.ts";
import { run as streamClassDefault } from "./_helpers/new_cast_builtin_shadow_10589/d_stream_class_default.ts";

console.log("headers fn named:", headersFnNamed());
console.log("headers fn default:", headersFnDefault());
console.log("headers class named:", headersClassNamed());
console.log("headers class default:", headersClassDefault());
console.log("emitter fn named:", emitterFnNamed());
console.log("emitter fn default:", emitterFnDefault());
console.log("emitter class named:", emitterClassNamed());
console.log("emitter class default:", emitterClassDefault());
console.log("stream fn named:", streamFnNamed());
console.log("stream fn default:", streamFnDefault());
console.log("stream class named:", streamClassNamed());
console.log("stream class default:", streamClassDefault());
