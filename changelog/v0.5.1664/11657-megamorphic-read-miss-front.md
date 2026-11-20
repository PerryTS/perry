A generic property read keeps only the ShapeId compare and the slot load
inline. Its miss makes one GC-leaf call, `js_object_get_field_ic_front`, which
answers a polymorphic way, a spill entry, or a latched megamorphic site whose
slot guess the receiver's own shape record confirms (one POSBOUND bound
compare and one key-atom word compare, with a bounded second chance over the
first 32 positional keys that re-aims the guess). Only what the front declines
reaches the collecting slow entry, which also asks the inherited-read cache
for a never-primed site. Nothing is spilled or relocated across the front
call, and the site reads its agent's shape directory without a call on ELF
executables, Windows x86-64 and Apple aarch64. The shape record grows from 40
to 48 bytes; tsc's `.text` shrinks by 9%.
