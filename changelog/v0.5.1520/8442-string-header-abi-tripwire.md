### fix(ffi): guard the published string-header ABI against runtime drift

`perry-ffi` now publishes a string-header ABI revision paired with the runtime's
exported revision symbol, and tests pin both revisions and the 20-byte layout.
Out-of-tree native wrappers can fail loudly on an incompatible runtime instead
of reading corrupt string payloads. The borrowed string/byte helpers also now
document that moving-GC borrows must be copied before the next runtime allocation.
