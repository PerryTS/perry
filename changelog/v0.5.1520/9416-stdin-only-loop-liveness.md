### Fixed

- A program whose only pending work is a `process.stdin` read no longer exits
  before the bytes arrive (#9416). `process.stdin` reached as an object — an
  alias, a parameter, or a field — files its listener in perry-runtime's own
  stdin registries; #9399 taught perry-stdlib's `js_stdlib_has_active_handles`
  about those lists, but such a program links runtime-only, where the symbol the
  generated event loop calls is perry-runtime's trampoline and the stdlib arm is
  unreachable. The trampoline now consults `stdin_listeners_keep_loop_alive()`
  itself, so stdin-driven filters, REPLs and stdio transports stay alive exactly
  as long as Node keeps them (and no longer: `pause()`/`unref()`/`destroy()` and
  EOF-plus-`'end'` still release the loop).
