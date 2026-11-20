**perf(codegen): guard a run of property reads spelled across statements once (#10884 step 4b, slice 2).** `const a = o.a; const b = o.b; const c = o.c;` is now one region with one shape guard instead of three separately guarded reads.

The guard itself (state word, R1/R2, bounded prime, miss edges) moves into the shared `expr/region_guard.rs`, so the within-expression slice and this one match over a single guard with a single soundness argument. Slice 1's measurements are unchanged by the move.

This slice can't duplicate the run into fast and slow copies: `Stmt::Let` would allocate an entry alloca per copy. It can't phi the loaded values either, because in the bail arm a generic read can reach a getter and move the heap. Instead each binding is declared once through the ordinary `Let` path. The fast arm stores each loaded slot into its binding immediately, so every value is rooted before the next read, and the bail arm assigns the same slots in source order.

Nothing is hoisted across an operator, so there is no type condition: string- and object-valued fields qualify too. The kill switch is `PERRY_REGION_READS=0`.
