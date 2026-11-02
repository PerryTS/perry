**fix(codegen): point the module-global `Ptr<Shape>` denial at its real issue.**
`MODULE_GLOBAL_ISSUE` in `expr/slot_rep.rs` — the issue number the optimiser
report prints when a module-level binding is denied a canonical slot — cited
**#7109**. That is a different mechanism: #7109 is the module-init /
program-entry *context* gate, which `MODULE_INIT_CONTEXT` in the same file
still cites correctly. #7109 is closed, and so is #10774, which lifted that
gate.

So every reader who followed the denial's own pointer landed on a closed issue
about something else, and could reasonably conclude the module-global class was
already handled. That is not hypothetical: one optimisation pass recorded
module-global storage as "less important" on exactly that reading, and a
separate campaign spent a day repeating "#7109 is the blocker" on inherited
belief before checking the issue state.

Now points at **#10803** (`Ptr<Shape>` is denied to three storage classes:
module globals, function parameters, and locals escaping into a module global),
with a comment recording why the old pointer was wrong so the correction is not
silently reverted.

Deliberately unchanged: `MODULE_INIT_CONTEXT`'s `#7109`, which is accurate to
that rule's subject, and the `#7109` fixture string in `opt_report/render.rs`'s
test helper. Only the live user-facing pointer for the module-global storage
class moved. `the_context_gate_is_reported_when_every_value_rule_passed`
asserts the `MODULE_INIT_CONTEXT` pointer and still passes unchanged, which is
what confirms the two were separable.

Found by cross-session review while handing the module-global lane to another
campaign; the denial classes it names are that campaign's #10803.
