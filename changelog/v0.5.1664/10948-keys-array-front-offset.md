Fixed four sites computing a keys array's element base as `header + 8`
(#10939).

`array_front_offset` is `array_physical_capacity - capacity`, so logical element
zero sits past the header for any ordered keys array whose front has been
consumed — a dense-queue shift, a `GC_ARRAY_NAMED_PROPS` reserve, #9019's
reserved-floor seed, or a size-class round-up on its own. Four sites computed
the base by hand instead of asking `keys_array_dense_slots` /
`array_elements_ptr`:

* `object_ops/keys_array.rs` — clone-before-mutate for `defineProperty`
* `field_set_by_name/tail.rs` ×2 — clone-before-push on `[[Set]]` growth
* `field_get_set/ic_miss.rs` — the key scan on the miss path (read only)

The three copy sites are worse than a bad read. Their destination publishes its
prefix as a region the collector walks as heap pointers, so copying from the
wrong base does not merely lose a key — it promises the collector that
`ArrayHeader` and front-reserve words are pointers. Two symptoms, in order: a
missing property now, and a SIGSEGV inside a later collection whose backtrace
names something unrelated (one landed in a `URLSearchParams` shape probe).
