Local three-second sampling diagnostic, not a qualified CPU/RSS measurement.
The worker and sampler both exited successfully; the worker hash matches the
record-output release. Of 2,206 main-thread samples, 1,958 enter stringify.
About 511 samples are under scalar-piece floating-number formatting, 423 stop
in string-piece planning, 321 in record emission, and 182 in memmove. The six
field fixture stores integer-valued id/score as f64 and reaches Ryū here.
Forty samples enter loop GC; that observation is specific to this small-record
workload and does not describe large-payload collection cost.

Next experiment: use exact-integer formatting below 2^53 in scalar_piece,
preserving negative zero, fractional values and shortest-roundtrip boundaries.
