**A function object points to its body's static `JsFunctionInfo`; the
closure-body registry is gone.** Every fact a caller needs about a body —
its code address, the parameter count a caller pads to, the rest kind and
its fixed prefix, `.length` and its declared fallback, the arrow / strict /
async / generator / built-in bits, and the compiler-private trusted-direct
and versioned-loop clones — is one immutable record per body (perry-abi
`JsFunctionInfo`), and the function object's header word at +8 points to it.
A fact is one load from the object; nothing is looked up by code address.

- Codegen emits each body's info as a constant next to the body
  (`@<body>$info`), and every allocation names the info. A module declares
  another module's info `external` instead of copying it, so a body has one
  info and one singleton function object.
- Runtime natives and perry-ffi addons define the info as a `static` from
  the body's typed pointer: `fn_info!(body, n; ...)` in the runtime and
  stdlib, `perry_ffi::js_function_info!(body, n; ...)` in addons. The
  parameter count comes from the body's type; a body of any other signature
  does not compile.
- Deleted: `CLOSURE_BODY_REGISTRY`, `TRUSTED_TARGETS`, `DISPATCH_RECENT`, the
  twelve `js_register_closure_*` entries (arity, rest, rest-and-arguments,
  synthetic arguments, length, arrow, strict, async, generator, async
  generator, trusted direct, versioned loop) with their ~885 references, and
  every module-init registration call codegen emitted for them. A class
  constructor's rest position travels with its class constructor flags.
- perry-ffi 0.6: `alloc_closure(info, captures)` takes
  `&'static JsFunctionInfo`; `register_closure_arity` and
  `register_closure_rest` are removed (the facts are builder calls on the
  info: `with_rest(n)`, `with_length(n)`, ...).
- `ClosureHeader::code()` proves the cell is a live function object before it
  loads through the info, so comparing an arbitrary receiver's code against a
  known body stays a compare (it used to read the raw word at +8).
- `scripts/check_js_body_call_funnel.py` refuses a closure allocator handed
  a raw code pointer, an allocator declaration whose first parameter is not
  `*const JsFunctionInfo`, and any `js_register_closure_*` name or deleted
  code-keyed table anywhere in the crates.
- `test-files/test_gap_fn_info_facts.ts` reads each info field through
  generic calls (the function travels as a value), against node.
