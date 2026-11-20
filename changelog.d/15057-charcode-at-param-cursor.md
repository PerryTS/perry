`s.charCodeAt(i)` on a short non-ASCII string parameter no longer decodes
from the start of the string on every read. A parameter the function never
rebinds reads through a stack cursor of byte and UTF-16 offsets, so a scan
decodes each code point once: the UTF-8 encode loop from #14973 runs in
195 ms instead of 730 ms.
