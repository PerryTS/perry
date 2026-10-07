The new raw stores carry GC store-site audit markers, the new header reads use
the `addr_class` helpers, and the stream emitter's shape fast paths read keys
through `heap_string_header`. The structured-clone writer returns an empty
string for a string payload in the handle band instead of reading memory there.
