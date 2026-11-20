`bun:sqlite`'s `query`, `run` and `transaction` now work through an indirect
receiver.

`dispatch_node_sqlite_database_method` has always implemented all three — the
same registry backs both bindings — but they were missing from the dynamic
handle-dispatch gate, so they never reached it. Codegen's static
`NATIVE_MODULE_TABLE` entry covers `db.query(...)` only while the receiver's
class is provable; as soon as the database reaches a call site through any
indirection — a field, an interface-typed parameter, a generator return, a
driver object — the call fell to the dynamic tower and silently returned
`undefined`.
