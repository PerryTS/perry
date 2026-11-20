`String.prototype.codePointAt` is no longer O(n) per call on strings containing
non-ASCII characters. It now uses the same lazy UTF-16 index `charCodeAt` and
bracket indexing were moved to in #10067, so a sequential scan is linear rather
than quadratic. A natively compiled `tsc` goes from 658 s to 85 s on a two-line
input (#10656).
