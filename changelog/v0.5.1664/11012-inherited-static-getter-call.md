### Fixed

- `C.g(args)` where `g` is a static getter inherited through a RUNTIME-resolved
  heritage (`class G extends make() {}`, and any further subclass) now reads the
  accessor and calls its value instead of throwing `TypeError: g is not a
  function` (#10893). The lookup is a new arm in the runtime's class-id
  parent-chain walk (`js_class_static_method_call`), which is the only place
  that parent edge exists — codegen sees only the static `extends_name` chain.
