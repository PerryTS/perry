Class-method value reads build no key string. Reading a class method as a
value (`this.parse` in Zod's `this.parse = this.parse.bind(this)`, `obj[k]`
for a method name) first checks for an own data property of that name. The
check built a GC string from the method name's bytes only to compare it with
the receiver's keys. It now compares the bytes with the keys in place
(`own_data_field_by_bytes`, which shares one body with
`own_data_field_by_name`). `js_class_method_snapshot_bind` drops the same
string and its handle scope.

On Zod ×5000 this removes 2.12 M string allocations (2.19 M → 74 k) and
4.1% of instructions.

Tests: `test_gap_method_lookup_own_shadow` (own shadowing, identity, computed
keys, long and unicode names; matches Node) and runtime tests asserting that
both the shadowing hit and the miss allocate nothing. Re-introducing the key
string fails them.
