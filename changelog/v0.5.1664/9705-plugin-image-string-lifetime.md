**A compiled plugin no longer lends its rodata to a registry that outlives it.**
#9188's borrowing `js_register_function_name_static` /
`js_register_function_source_static` hand the registry a `@.str.N`
`private unnamed_addr constant` instead of copying, on the contract that those
globals live for the life of the image. Image lifetime and process lifetime are
the same thing for an executable and are not for a plugin: perry compiles
TypeScript to a `dylib` too (`codegen/entry.rs` emits its
`perry_plugin_abi_version` / `plugin_activate` shim), and `perry_plugin_unload`
ends in `dlclose`. The emission was unconditional, and neither registry has an
unregister path — `perry_plugin_unload` clears plugin hook registrations only —
so after an unload the maps retained `(ptr, len)` pairs into unmapped memory,
and the next `fn.name`, `Function.prototype.toString()` or error stack frame
that resolved one read it. Being address-keyed, a later image mapped over the
same range would collide silently rather than fault.

`emit_string_pool` now picks the spelling from `output_type`: an executable
keeps the borrow (where all the volume is — 72,713 registrations on the
compiled Claude Code TUI), while `dylib` and `staticlib` emit the copying
spelling. `staticlib` is included because its objects are linked into whatever
consumes them, which may itself be a plugin. Both spellings are declared in
`runtime_decls` again.

`registration_spelling_follows_output_kind` pins both directions — losing the
executable's `_static` silently restores the startup copy the optimisation
removed, and losing the dylib's copy reintroduces the use-after-free. It was
verified to fail against the unconditional emission before being committed.
