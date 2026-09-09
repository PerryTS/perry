# Superseded source-length candidate R1

This quiet window is diagnostic evidence, not an accepted release result. The
candidate reuses the full input header's UTF-16 count for a dominant unescaped
string token with bounded ASCII surroundings. Its exact source and artifact
hashes are preserved in `source.patch` and `provenance.json`.

The focused rotating-input window improved ASCII from 108.041 to 95.621 µs and
Unicode from 148.298 to 81.300 µs, but small records regressed from 0.498084 to
0.503749 µs (+1.14%). All five candidate small-record CPU samples exceeded all
five reference samples. The complete original matrix also showed reproducible
small regressions on multiple object rows and the one-character stringify row.
These must not be dismissed merely because some are below a percentage screen.

R2 moves the large-token proof and counted allocation into an outlined helper,
leaving a simple size dispatch and the ordinary constructor in the small-token
arm. R2 requires its own matching build, correctness checks and measurements.
The reference in both R1 windows is release 0.5.1528 from PR #10032, not main.

Disassembly confirms a shared-path code-size change: `parse_string_value` grows
from 976 to 2,476 bytes and its stack frame grows from 96 to 144 bytes. The
ordinary constructor's count code was inlined into the candidate's common path.
The two archived assembly extracts identify the exact instructions and symbols.
This supports the outlining experiment; only R2 timings can establish its effect.
