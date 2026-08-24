---
category: Performance
title: Pack monomorphic method shape guards
---

Monomorphic direct-method guards now validate the contiguous GC, class, and
ShapeId header fields with two packed loads while retaining prototype
invalidation, pointer-range checks, ShapeId validation, and the generic
fallback. This reduces repeated dispatch proof work in hot method-call loops.
