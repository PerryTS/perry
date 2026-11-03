Collapse the monomorphic class-field GET tower to one runtime exit. The #5093
inline precheck and the fast slot load are unchanged; the guard call, the
nullish-throw diamond and the by-name fallback arms are replaced by a single
call to the existing `js_class_field_get_ic`, which performs the same guard,
load, fallback and throw. Per site that is 88 → 47 IR instructions, 3 → 1
runtime call sites and 6 → 1 RS4GC relocation reloads; the fast path no longer
crosses a statepoint, so it sheds the two relocation reloads it used to carry.
On @babel/parser `.text` shrinks 7.1 % and `.perry_gcmap` 2.0 %, with runtime
instructions and RSS unchanged and `interp.ts` 13 % faster; typed-feedback
guard counters are identical per site.
