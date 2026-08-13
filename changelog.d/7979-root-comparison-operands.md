**Fixed: comparison call results going stale while the other operand collected (#7979).**

Comparison lowering evaluated the left operand, evaluated the right operand,
and then consumed the original left SSA register. For an inline verdict such
as `observed() === expected()`, an allocating right-hand call could run a
copying minor after the left call returned a heap string. The collector rewrote
roots but could not rewrite that bare register, so `js_eq` later dereferenced
retired from-space in `js_jsvalue_equals`.

All comparison paths now use the shared selective operand-rooting scope. Each
operand that can hold a GC pointer is protected before later operands run,
re-read after evaluation, and kept rooted through the comparison dispatch;
proven non-pointer operands remain in their original registers and emit no
root traffic.

Codegen tests trace `js_eq`'s arguments back through pure LLVM operations and
assert that an object-valued left operand is re-read below an allocating right
operand, while a proven-number left operand is not. The original
`test_gap_gc_define_property_descriptor_rooting.ts` witness is restored to
inline comparisons and registered in the moving-GC corpus; before the fix its
string operand faults under retired-from-space protection, while the rooted
form remains byte-exact with Node under scheduled moving collections.
