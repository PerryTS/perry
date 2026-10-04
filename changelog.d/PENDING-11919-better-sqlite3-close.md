**better-sqlite3 connections now close eagerly.**

`Database#close()` now drops the native SQLite connection immediately instead
of only validating its handle. Existing statements and later database
operations report better-sqlite3's closed-connection `TypeError`, and repeated
open/close cycles no longer retain file descriptors or connection memory.
