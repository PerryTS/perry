`CreateDataProperty` (public class fields, and the other runtime callers of
the operation) defines from a Property Descriptor record instead of building a
four-key JS descriptor object and decoding it back through
`Reflect.defineProperty`. A class field on a callable class instance (a bound
function given the class prototype, as ArkType's `Callable` builds) declines
every shape-owned define and paid both steps: 24k instructions per field,
1.57 G instructions of the Supabase CLI's `--version`. It is now about 10k.
Because nothing is read back through `%Object.prototype%`, an inherited `get`
or `set` there no longer turns a class field into an invalid accessor
descriptor; `test_gap_class_field_define_record` pins that and the field
attributes on callable and plain receivers.
