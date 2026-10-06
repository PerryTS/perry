RegExp operations on typed locals and literals now use ordinary method and property sites. Patched or own `test`/`exec` methods and replaced `source`/`flags` getters are observed through the same path as untyped receivers. Removes the dedicated instance-operation HIR nodes and their compiler consumers.

Generic builtin test calls share the rooted raw-string execution boundary. Heap-string arguments avoid a redundant coercion trap and root scope; effectful coercions and overridden exec methods retain their observable ordering. Moving-GC and allocating-operand cases cover the rooting migration.
