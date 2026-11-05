---
category: Performance
title: Specialize imported methods that capture this
---

ESM import and re-export metadata now carries producer-proven method
eligibility, allowing guarded direct calls into stable class methods that use
`this`. Generic dispatch remains available for shadowed or mutated receivers,
including prototype replacement, deletion, and recreation.
