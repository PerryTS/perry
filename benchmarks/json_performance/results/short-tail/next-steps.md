Keep the full 38 CPU / 74 peak RSS / 36 retained RSS goal and every unresolved
regression. This checkpoint does not meet the no-regression requirement.

1. Limit bounded short-word scanning to string_piece, restoring the original
   shared scanner. Keep the safe initialized String plan seeds. Check the
   short-only helper directly against the scalar oracle and at a guard page.
2. Rebuild with the same four-package settings and pinned application object.
   Compare every row to escaped-count, and preserve small-record stringify and
   long-ASCII/Unicode parse comparisons to this checkpoint as additional anchors.
3. Inspect code and process instructions, then run 172 runtime tests, 38 pinned
   compiled comparisons, 34 moving-GC runs, all source hashes, and complete
   quiet full/paired CPU and RSS matrices. Do not infer performance from code size.
4. Continue the broader parse/object allocation and large-record stringify work
   after recovering regressions. JSON scratch remains native; inputs are rooted
   and rederived across output allocation. GC production policy is unchanged.
