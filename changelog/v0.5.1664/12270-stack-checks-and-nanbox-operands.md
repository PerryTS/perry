The stack check at the start of every compiled function is shorter. On x86-64
Linux executables it is now one compare of the stack pointer against the
agent's stack limit and one branch (it was four instructions), because the
limit is read as a local-exec thread-local and the stack pointer through
`llvm.read_register`. The entry overflow path ends after the throwing runtime call and
keeps nothing live across it, so it no longer spills and reloads the
function's arguments or jumps back. Deep recursion still throws a catchable
`RangeError: Maximum call stack size exceeded`.

Compiled x86-64 Linux executables are smaller. Tag tests and masks on JS values
used to encode their 64-bit NaN-box constants as a 10-byte `movabs` at almost
every use. The runtime now holds these constants in one read-only table, and
compiled code reads them from it as memory operands, which the processor folds
into the compare, `and` or `or` that uses them.
