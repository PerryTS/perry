## Immutable string-key object-write fast path

Object-write loop specialization now recognizes an immutable local initialized
from a string literal (for example, `const key = "x"`) as a static property
key. It reuses the guarded write PIC and bounded loop fast path without
retaining a movable runtime string pointer. Mutable or computed keys continue
to use the fully generic property-write path.
