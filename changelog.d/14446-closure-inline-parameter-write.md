A one-expression arrow local that is only called (`const f = (a) => ...; f(x)`)
is reduced to its body at each call. That rewrite put the argument where the
body reads the parameter, which changed the program in two cases: a body that
writes its parameter (`(a) => (a = a + 10)`) wrote the caller's local instead,
and an argument local that the body writes through another path (`(a) =>
bump() + a` with `bump` writing the argument) was read after that write
instead of at the call. The rewrite now applies only when the body writes none
of its parameters and every argument is a literal or a local that nothing in
the module writes; a global read is no longer treated as a fixed argument.
