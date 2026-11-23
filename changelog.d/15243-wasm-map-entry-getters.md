On `--target web`, `new Map(iterable)` and `new WeakMap(iterable)` read each entry's `0` and `1` keys through compiled getters, and a bare `new WeakMap(...)` constructs a real WeakMap.
