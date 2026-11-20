`Object.freeze`, `Object.seal` and `Object.preventExtensions` no longer write
into memory in front of values that have no GC header. They recorded their
flags in the eight bytes preceding the value, which is where a real object
keeps its header — but several values perry hands to JavaScript have no header
there, so the write landed in whatever the allocator had put in front of them.
Freezing one particular value (the placeholder object perry returns for an
unresolved module or an unknown method) wrote into read-only memory and
crashed the process; freezing a `Symbol.for(...)` silently modified a
neighbouring allocation.

The three operations now check that the value is one the allocator actually
owns before recording anything, instead of checking only that its address is
large enough. A value that fails the check is left alone and returned, which
is what already happened for the handles the old check did catch.
