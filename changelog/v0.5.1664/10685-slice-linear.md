`String.prototype.substring` / `slice` / `substr` no longer walk from byte 0 to
resolve their start offset on strings containing non-ASCII characters. They now
use the same lazy UTF-16 index the other accessors use, so slicing at increasing
offsets — every tokenizer's access pattern — is linear rather than quadratic. A
natively compiled `tsc` goes from 85 s to 7.8 s (#10685).
