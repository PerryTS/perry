The stack check at the start of every compiled function is shorter. On x86-64
Linux executables it is now one compare of the stack pointer against the
agent's stack limit and one branch (it was four instructions), because the
limit is read as a local-exec thread-local and the stack pointer through
`llvm.read_register`. The overflow path calls a `noreturn` runtime entry and
keeps nothing live across it, so it no longer spills and reloads the
function's arguments or jumps back. Deep recursion still throws a catchable
`RangeError: Maximum call stack size exceeded`.
