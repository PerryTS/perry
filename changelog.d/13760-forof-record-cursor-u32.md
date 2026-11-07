Store the for…of IteratorRecord cursor over an array in a native u32 slot
instead of a double. The cursor qualifies only when every write is the
record's own initializer or the `++` of its loop update, which runs only
below the array's u32 length; in iterator mode the update never runs and the
cursor stays 0, so the proof holds when the array iterator protocol is
patched. The loop update becomes `add i32` with `uitofp` only at boxed uses.
Tests cover the collector (record loops, literal records, nested and
break/continue loops, every disqualifying write) and the emitted update block.

The cursor consumers take it as an integer: the indexed head compares it with the header length word (`icmp ult i32`; the Number compare only on the cold length read), record reads index with it directly, and `c++` materializes one Number view instead of two.
