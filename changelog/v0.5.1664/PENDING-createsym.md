Skip repeated empty-symbol-key scans in descriptor reflection, defineProperties and
Object.assign. A live ordinary shape derives symbol absence from its immutable key
prefix at publication; mutable lists and exotic receivers retain the existing
enumeration. Every caller still receives a fresh array at the existing allocation
point, preserving collection timing.

Reuse the existing exact keyless birth-record lookup before minting an Object.create
birth shape, and validate the prototype once at the shared create boundary. Function
prototypes retain their identity. No cache, registry, side table, latch, special name
check or alternative allocation path is added.
