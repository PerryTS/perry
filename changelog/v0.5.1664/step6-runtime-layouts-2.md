Generator results now use the shared runtime iterator-result descriptor, removing their separate packed key/hash allocation and stores.

Standalone layout, iterator and stdlib fixtures share gc::initialize_runtime_fixture for GC and runtime layout startup. Added a generator layout/GC test and five Node parity fixtures covering generator results and constructor property/prototype semantics.
