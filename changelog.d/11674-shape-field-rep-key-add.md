Adding a property now records its field representation in the shape: a key-add
of a Number into an inline slot gives the new shape an `F64` lane, any other
value (or an overflow slot) an `Any` lane, and the predecessor's lanes carry.
The transition cache needs no new key bit: a cached edge serves a value only
when its target's lane admits it (an `F64` lane refuses a non-Number, a target
with a deprecated lane never serves, so new objects converge on the normalized
shape). The by-name cache-hit writers and `js_object_set_field` now store
through the checked funnel, so an `F64` slot always holds a canonical double.
