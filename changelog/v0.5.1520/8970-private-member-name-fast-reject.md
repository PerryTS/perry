The generic property read and write no longer UTF-8-validate every key to
discover it isn't a private class member.

`private_member_storage_name` sits at the top of both `js_object_get_field_by_name`
and `field_set_by_name`, so every property operation in the program pays it —
and it reached its verdict through `str_from_string_header`, which validates
the WHOLE key as UTF-8 before the prefix compare can reject it. In a
computed-key read loop that measured ~7.5% self time (plus its share of
`from_utf8`), all of it spent proving that `"k123"` does not begin with `#`.

A storage name always starts with `#` and is longer than the
`#<perry:private-member:` prefix, so a length compare and one byte settle it
for every ordinary key. Keys that pass the filter still take the original
path, so private members — and the rare `#`-prefixed ordinary key — behave
exactly as before.

Interleaved A/B, min-of-15 at quiet load: read loop 38 → 34 ms (−10.5%),
combined overwrite loop 82 → 76 ms (−7.3%), write loop 44 → 43 ms; means move
the same direction on all three. Output on a private-member differential
(fields, static private counters, private methods, private getters, `#x in o`,
subclassing, and an ordinary key named `#<perry:private-member:1:x>`) is
byte-identical to pre-change perry.
