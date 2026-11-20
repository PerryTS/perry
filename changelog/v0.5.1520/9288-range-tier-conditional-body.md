**A conditional-count loop now gets a packed clone whatever its bound is spelled**
(#9275). The packed-f64 range tier rejected any `if (...) c++` body, so
`for (k = 1; k < N; k++) if (a[k] > a[k-1]) c++` was compiled two entirely
different ways depending on whether `N` was written `4096` or `a.length`: 31 ms
against 4 ms for identical work, which is the difference between losing to node
by 3.1x and beating it by 2.5x.

An `if` died in a different place in each of the tier's two walkers. The
single-statement walker opens `let [Stmt::Expr(expr)] = body else { return
false }`, so a `Stmt::If` fails the destructure before any arm runs; the dense
walker has arms for `Let`, `LocalSet`, `Update` and a generic `Expr`, and no
`Stmt::If`, so it reached the trailing `_ => return false`. The versioned tier
meanwhile accepts the shape at `stmt_is_packed_f64_loop_safe`'s `Stmt::If` arm,
which recurses condition and both branches — two tiers with different admission
power, and which one claims the loop decided by how the bound was written. It is
the mirror image of #9259, where the `arr.length` spelling was the slow one.

The new arm goes in the dense walk and only there, because dense mode's own
safety argument carries over unchanged: its loads have no side exits, so an
iteration runs entirely in the fast copy or entirely in the slow one and a
branch cannot leave a half-applied iteration behind. The classic mode cannot
take this — it permits a hole-read side exit that re-executes the iteration,
which is precisely why it insists on a single statement whose one side effect
happens last. That reasoning now lives in the arm, so the classic walker does
not get "fixed" the same way later.

`written` and `accesses` are threaded into the branch walks rather than rebuilt
per branch: a scalar assigned inside a branch still has to shadow-check against
the tracked arrays, and the tail check runs once on the merged set. Reads from
both branches are recorded, so the entry guard validates the union of the
windows rather than the taken path's — conservative in the safe direction, since
a window only reachable on the untaken branch can cost the clone and never
correctness. `break` and `continue` inside a branch stay rejected, with a test
pinning that such a loop still compiles and returns node's answer through the
generic path.

Measured on a 4096-element array, self-timed min of 5, every timing paired with
a `packed_f64.*` block count from the emitted IR: `if (a[k] > 0) c++` under a
literal bound goes 31 ms to 12 ms (0 packed blocks to 18), and the offset form
`if (a[k] > a[k-1]) c++` goes 47 ms to 25 ms. The literal-bounded `c += a[k]`
body and the `arr.length`-bounded conditional are unchanged controls at 8 ms and
4 ms, which is what makes the zeros above attributable to the body shape rather
than to the bound. Output is node-identical on every fixture.

The range tier's clone is still around 3x the versioned tier's for the same
body, so this narrows the spelling gap from 7.75x to 3x rather than closing it.
A float accumulator whose RHS reads a tracked array (`c += a[k]`) also remains
unadmitted by the dense walk — not because of the conditional, since a plain
`c += a[k]; c += 1.0;` body is rejected identically, but because `c + a[k]` can
lower to a dynamic add and the accumulator needs a numeric proof this walk does
not have. Both are recorded rather than left to be rediscovered.
