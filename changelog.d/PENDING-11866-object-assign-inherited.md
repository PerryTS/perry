### Fix Object.assign inherited property semantics

`Object.assign` now uses receiver-aware strict `[[Set]]` for string and symbol
keys, invoking inherited setters and throwing for inherited read-only or
getter-only properties. It snapshots ordinary source keys and rechecks each
descriptor before reading, so setters that delete or redefine later source
properties affect the copy correctly. Object-literal spread retains its
separate property-definition path.

Regression coverage: `test_gap_11866_object_assign_inherited.ts` (#11866).
