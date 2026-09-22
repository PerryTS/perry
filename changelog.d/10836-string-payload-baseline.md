Lowered `inline-offset | perry-runtime` from **350 to 349** in
`scripts/string_payload_access_baseline.txt`.

The ratchet is bidirectional by design — "a category may never increase in a
crate, and **a decrease must lower the baseline in the same change**" — so
removing a site without recording it leaves the gate holding at the looser
number and silently re-admits one.

The net −1 is the sum of four real movements, not a single deletion:

| file | before | after |
|---|---|---|
| `object/field_get_set/get_field_by_name_tail.rs` | 19 | 17 |
| `object/field_get_set/ic_miss.rs` | 4 | 3 |
| `gc/tests/handle_bound_method_name.rs` | 1 | 2 |
| `text.rs` | 4 | 5 |

The two decreases are the point of this PR pair: making `TextEncoder` /
`TextDecoder` instances and the timer handles into **ordinary objects** removes
special-case branches from the property-read tail, and those branches carried
open-coded payload reads. The two increases are in the rewritten
`with_mut_ptr`-scoped bodies and the reformatted test.

Caught before the build by the lint-derived preflight rather than 12 minutes
into `lint`. Verified per file with
`string_payload_access_inventory.py --list` diffed against `origin/main`, so
the number is attributed rather than merely accepted.
