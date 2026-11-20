**Objects / GC:** the class-field typed-feedback guards, the runtime slot-store INT32 canonicalization and the
object-array numeric write guard answer "does this slot hold a raw double" from the lane of the receiver's shape
instead of the per-object typed layout; `PERRY_VERIFY_TYPED_INTACT` and the field-representation typed-layout
cross-check are gone (charter step 5, P4 flip).
