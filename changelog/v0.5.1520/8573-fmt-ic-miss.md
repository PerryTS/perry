**`lint` is red on `main`: `ic_miss.rs` is unformatted.**

`lint` fails at its "Check formatting" step on `main`. #8572 converted `field_get_set/ic_miss.rs`'s raw-handle reads to the scoping combinators but landed the result unformatted, so `cargo fmt --all -- --check` reports a diff:

```
Diff in crates/perry-runtime/src/object/field_get_set/ic_miss.rs:1315:
-                key.with_const_ptr(|k| {
-                    crate::object::js_object_set_field_by_name(o, k, 42.0)
-                })
+            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 42.0))
```

This is `cargo fmt --all` output only — 12 lines in that one file, no other file touched, no semantic change. `cargo fmt --all -- --check` exits 0 afterwards.
