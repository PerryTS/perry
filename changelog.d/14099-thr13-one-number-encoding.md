A Number now has one encoding in every JS value: the raw double. 32-bit
integers produced by perry-ffi (`from_int32`), stdlib SQLite INTEGER columns,
the codegen i32 boxing and the Map/Set i32 helpers used to store the compact
INT32 word, so a better-sqlite3 row field gave `Object.is(row.v, 5) === false`
(node: true). The ECS array-subclass tier keeps entity ids as raw doubles and
converts on load. A field the program facts prove holds only Numbers is now
read as a plain load (r5: dynamic multiplies 12 to 6, about 2% fewer
instructions). Debug builds assert that no INT32 word is stored into an
object field.
