An `if` with no `else`, inside a loop, whose `then` contains a loop has its
condition wrapped in `llvm.expect.i1(cond, false)`, so the skip of the nest is
the fall-through and entering the nest costs one taken branch. No hint outside
a loop, with an `else`, or without a nested loop. Test:
`stmt::if_loop_hint_tests`.
