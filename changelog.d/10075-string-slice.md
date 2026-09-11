Fix `String.prototype.slice`, `substring`, and `substr` at UTF-16 boundaries
inside astral characters, preserving either surrogate half and pre-existing
lone surrogates with consistent length and WTF-8 metadata.

Avoid repeated full-suffix copying in eligible native parse loops by keeping
non-escaping suffix locals as rooted sources with scalar UTF-16/byte cursors.
Escaping strings retain their flat storage and independent lifetime. Includes
Unicode boundary, aliasing, moving-GC, and unchanged workload benchmark coverage
for #10061.
