The JSON string serializer and the worker structured clone now classify a
pointer in the handle band with the `addr_class` predicates. A string pointer in
that band reads as null or empty instead of being dereferenced. A small native
handle posted to a worker now throws `DataCloneError` instead of arriving as
`undefined`.
