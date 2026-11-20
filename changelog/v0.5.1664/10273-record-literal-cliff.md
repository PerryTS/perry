Fix the mid-size record-literal compile cliff while retaining static record
shapes and typed property reads (#10173, OpenCode bring-up #10107). Constant
record trees now materialize from read-only descriptors using the same class
IDs, rooted keys, typed shape IDs and field masks as ordinary construction.
Wide synthetic constructors share a strict assignment helper with precise
operand roots instead of duplicating huge field-store bodies.

On the Linux LLVM 22 audit, 3,200 records compile in 0.63 seconds versus 95.49
seconds, with peak RSS falling from 1,440 to 238 MiB and module `.text` from
6.30 MB to 6.02 KB. Five alternating runtime runs preserve both checksums and
numeric performance; record reads improve. Regression coverage checks bounded
IR, the below-cutoff compile case, fresh mutable values, and real GC movement.
See `benchmarks/large_json_literals/measurements-10173.md` for phase attribution,
the fast-emission control and standalone OpenCode module measurements.
