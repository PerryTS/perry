A plain `Map` or `Set` answers `GetIterator` with its builtin iterator.

`subclass_backing_for_default_iteration` claims only instances carrying a hidden backing field, so a plain collection fell through every arm of `js_get_iterator` to the generic `[Symbol.iterator]` property lookup plus a dynamic method call — the shape a `for…of` over a small collection hits on every iteration.
