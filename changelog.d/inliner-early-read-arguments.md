- **perf(transform):** the inliner again substitutes a written local for a parameter when the read cannot move past
  anything that could change it. #14454 bound every argument that is not a literal or a never-written local to a
  `let` before the inlined body; a call in a loop condition has no place for that `let`, so `while (onBoard(source))`
  with `source += delta` in the loop stayed a real call (chess `attacked`: 8 calls, each ending the held base of
  `this.board`; search +5.2%, perft +7.8%, legal +4.6% instructions). The rule (`WriteCensus::substitutes_arg`, scan
  in `arg_read_order`) now has a second case: a read of any local is substituted when no later argument can run code
  or store to it, and every read of the parameter in the body comes before anything in the body that could (a call,
  `new`, a property or element access, an operator's `valueOf` conversion, `await` / `yield`, a store to that local,
  any read inside a closure literal or in a loop that runs code). `(a & 0x88) === 0` and `a * a` qualify;
  `bump() + a`, `a.x + a`, `(a & 1) + a` and `f(m, m++)` keep the binding. `PERRY_INLINE_PURE_ARGS=0` restores the
  #14454 behaviour and is a compile-cache input.
