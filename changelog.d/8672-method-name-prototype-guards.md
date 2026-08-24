---
category: Performance
title: Restore method-scoped prototype guards
---

Prototype mutation now invalidates direct-call guards by method-name slot
instead of permanently disabling every method guard in the process. Hash
collisions remain conservative, and dynamic prototype replacement retains a
global fail-closed escape hatch.
