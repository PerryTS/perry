---
category: Performance
title: Index class prototype objects during startup
---

Class prototype membership checks now use an exact address index that is kept
in sync across registry updates and copying collections. This removes repeated
full-registry scans from property definition while large native module graphs
initialize.
