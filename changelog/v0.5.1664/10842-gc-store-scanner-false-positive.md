`scripts/gc_store_site_inventory.py` reported a **comparison** as a store:

```
crates/perry-runtime/src/object/inherited_read_cache.rs:375:
  raw heap pointer field store: (*meta).elements == 0
```

`(*meta).elements == 0` is a read. Four of the scanner's five store regexes
matched `=` without excluding `==`:

```python
RUST_FIELD_STORE_RE          r"\(\*[^)\n]+\)\.(keys_array|entries|elements)\s*="
RUST_PROMISE_FIELD_STORE_RE  r"\(\*[^)\n]+\)\.(on_fulfilled|on_rejected|next)\s*="
RUST_GLOBAL_INDEX_STORE_RE   r"\b([A-Z][A-Z0-9_]*)\s*\[[^\]]+\]\s*="
RUST_TLS_INDEX_STORE_RE      r"\(\*\w+\.get\(\)\)\[[^\]]+\]\s*="
```

while `RUST_POINTER_FIELD_STORE_RE`, two lines below the first, already carried
the `(?!=)` guard — so the correct form was sitting in the same block. All four
now have it.

**Not fixed with a marker or an allowlist entry**, which were the two paths the
gate's own error message offered. A `GC_STORE_AUDIT(...)` on this line would
document a store that does not exist, and would leave the scanner over-matching
for every other reader of `elements`, `entries`, `keys_array`, `on_fulfilled`,
`on_rejected` and `next`.

Verified the gate is not now vacuous, since loosening a regex is exactly how a
scanner stops being able to fail: a real unmarked store planted at the same
line (`(*meta).elements = 0;`) is caught — exit 1, naming the site — and the
tree is clean again with it removed.
