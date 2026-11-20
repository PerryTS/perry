### Fixed

- Preserve each UDP send callback's submission-time AsyncLocalStorage context, including distinct object stores and an empty store on an already-bound socket. Pending snapshots use the existing mutable GC scanner, and completion queueing roots values, restores ambient context and releases native snapshot ownership before rethrowing.
- Add a node-parity regression fixture while preserving callback receiver, arity, scheduling and error routing. Scoped verification corrects two existing async_hooks provider cases; the full module still has known failures and unrun cases.
