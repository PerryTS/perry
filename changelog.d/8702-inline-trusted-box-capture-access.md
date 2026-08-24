---
category: Performance
title: Inline trusted boxed capture access
---

Compiler-private exact-arrow clones now load validated boxed-capture pointers once at
entry and access their non-moving cells directly. Public closure bodies remain checked,
while the private path retains TDZ behavior and GC write barriers.
