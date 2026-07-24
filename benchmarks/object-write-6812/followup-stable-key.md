# Immutable dynamic-key follow-up

This follow-up measures the `const key = "x"; object[key] = ...` slice on top
of the bounded object-write loop fast path. The compiler was built from this
branch with isolated release artifacts; outputs were checked against Node.

| Cell | Node | Perry | Writes | Sink |
| --- | ---: | ---: | ---: | ---: |
| `key_stable_dynamic` | 138 ms | 124 ms | 120,000,000 | 122,876,400 |
| `key_alternating_dynamic` | 180 ms | 615 ms | 24,000,000 | 31,194,000 |

The stable-key cell now reaches the same bounded loop fast path as the static
key cells. Alternating keys remain intentionally generic and are retained as
the rejection/control case.
