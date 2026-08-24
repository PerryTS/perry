---
category: Performance
title: Reuse stable one-key for-in snapshots
---

Compiled `for...in` loops now reuse an ordinary object's immutable one-key shape snapshot when its own descriptors and prototype chain are stable. Other receivers keep the complete generic enumerator, including inherited keys, mutations, and Proxy behavior.
