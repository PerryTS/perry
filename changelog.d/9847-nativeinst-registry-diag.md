`PERRY_NATIVEINST_DIAG=1` reports every native-instance tag as it is created.

`register_native_instance` and `push_module_native_instance` are the only two
entry points through which such a tag can come into existence, so a diagnostic
on them cannot miss one the way a diagnostic on guessed construction sites can
— which is why it is placed there. One line per registration:

```
[nativeinst] REGISTER push_module name="O" -> child_process::Instance
```

The env var is excluded from the build-level cache, because a cached build
reuses the finished binary and never lowers HIR, so the report would print
nothing — and nothing is indistinguishable from "no tag was ever registered".

Off, the cost is one relaxed atomic load per registration.
