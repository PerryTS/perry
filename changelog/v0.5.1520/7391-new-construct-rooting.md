**Fixed** `new` read the callee from a pointer the instance allocation had
already moved, silently dropping a user-assigned `prototype`.

`js_new_function_construct` allocates the instance, then decodes the closure out
of `func_value` — computed *before* that allocation — and reads `CLOSURE_MAGIC`
off it through `is_closure_ptr`. An evacuating minor inside the allocation moves
the closure, so the decode names from-space:

```
+5208:  bl  js_object_alloc_with_parent    ; allocates
+5304:  ldr w8, [x24, #0xc]                ; faults
+5308:  cmp w8, #0x434c                    ; "CL" — CLOSURE_MAGIC
```

Without #7341's from-space quarantine this is quiet rather than fatal: the magic
check simply fails, the user-prototype link is skipped, and the instance gets the
wrong `[[Prototype]]` — `foo.prototype = new Array(1,2,3)` not taking effect,
with no crash and no diagnostic.

`js_new_function_construct_with_new_target` carries the identical shape with `nt`
in place of `func_value`, consumed by `constructor_prototype_bits` on the line
after the same allocation. Fixed as the same defect; the code notes it is not
independently reproduced.

`test_gap_learned_inline_sizing` is byte-identical to Node under quarantine (was
SIGBUS). 47 class/prototype/construct/new/reflect/inline gap tests pass; the two
that fail also fail on pristine `main`, and the unit suite's 4 failures sit
inside main's own 3/5/3 noise band (#7365).
