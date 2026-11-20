**A hoisted `const K = "a"` used as a property key no longer costs 7.3× the same read spelled `o.a`.**

`o["a"]` in source is already folded to `o.a` by the member lowering. But `module_const_fold` substitutes a hoisted const into the key position *after* that matcher has run, so the node stayed an `IndexGet` and was resolved by name at runtime on every read — UTF-8-validating the key, hashing it for the accessor Bloom summary, classifying the receiver and scanning the shape's key array.

Re-applying the same fold takes `O[K] + O[J]` from **1236 to 169 instructions**, exactly what `O.a + O.b` costs. It also fixes a spec divergence: `null[K]` and `undefined[K]` read `undefined` before, where node throws.
