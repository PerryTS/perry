### Fixed

- `fill(value, start, end)` on a `class X extends Array` instance ignored `start` and `end` — the instance-installed stub had arity 1, so `sub.fill(8, 1)` overwrote the whole array instead of the tail from index 1.
