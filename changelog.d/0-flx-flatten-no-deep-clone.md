Copying an HIR expression no longer recurses once per nesting level. The
derived `Expr::clone` recursed into every sub-expression with the frame of a
match over all 621 variants (15.7 KB per level in a debug build), so a
64-operand chain `o.k0 + … + o.k63` took about a megabyte of stack to copy.
The run-once view of per-unit facts copies the bodies it flattens, and the
test `a_closed_local_called_only_in_its_unit_costs_its_own_stores` overflowed
the 2 MB test thread in CI. `Expr::clone` now walks the tree in post order
with explicit stacks: each node is copied once with a hole at each child the
HIR walkers visit, and the holes take the children's copies. The view itself
no longer copies the module's statements before replacing them, nor the
functions it flattens, and a function literal it flattens moves into the view
instead of being copied twice.
