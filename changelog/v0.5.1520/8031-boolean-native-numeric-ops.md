### Proven booleans stay native in numeric operators

Boolean literals and locals with a live native-`i1` proof now convert directly
to `f64` for arithmetic and Boolean/number relational comparisons. This removes
their NaN-box round trips and `js_number_coerce` / `js_rel_*` calls while
keeping annotation-only or invalidated Boolean values on the dynamic path, so
values written through `any` retain JavaScript coercion and concatenation
semantics.
