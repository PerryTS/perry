### Fixed

- The production Next.js App Route dylib gate now exercises the **default**
  codegen backend. `tests/test_next_app_route_dylib.sh` pinned
  `PERRY_LLVM_INPROCESS="${PERRY_LLVM_INPROCESS:-0}"`, which selected the text
  transport and — because `${VAR:-0}` cannot express "unset" — left no way for
  the gate to run the configuration users actually get. The pin was reasonable
  while #8228 stopped the native in-process path compiling five of this
  fixture's modules; #8241 fixed that. The variable is now forwarded only when
  the caller sets it, so an explicit backend can still be selected for a
  bisection while the unset default reaches the compiler unchanged.

  Measured on `183d30c53a` before landing this: with the native backend the
  fixture compiles 104/104 modules and the gate passes 100/100 verifier
  repetitions twice, with `freeze/LLVM pipeline started` appearing 5× per
  compile (the five split modules #8228 could not build) versus 0× on the text
  path — so the gate now demonstrably runs the backend it claims to.
