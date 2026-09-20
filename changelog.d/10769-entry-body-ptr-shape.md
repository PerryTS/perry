**`Ptr<Shape>` is no longer disabled in entry bodies for a bug that was fixed in the runtime.**

`RepselContextFlags::derive`'s `Entry` arm forced `allows_ptr_shape: false` because *"#6991 is an open rooting bug in exactly that position"*. #6991 was closed by #7249, which put `populate_global_this_builtins` inside a `GcSuppressScope` — a runtime fix. The gate has been guarding a bug that no longer exists, and a shape proof in an entry body was being made, counted as a win, then dropped at every access site.

A module-level `const` with its loop at module level goes **110.00 → 88.99 instructions per iteration (−19.1%)**. The same body inside a function is the control and correctly does not move.
