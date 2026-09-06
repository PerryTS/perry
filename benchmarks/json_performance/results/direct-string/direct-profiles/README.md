# Direct-output diagnostics

Three-second `sample` captures, separate from default performance standings. All three processes complete with default GC and the quiet gate passes. Binary hashes and exact commands are in `provenance.json`.

In the numeric-array parse captures, the tape builder dominates both binaries: 1,638 main-thread samples in the reference and 1,722 in the candidate. These are distributions from separate captures, not paired timings. The seven-process timing repeats confirm a 17.6% candidate slowdown. Process instruction counts differ by about 0.04% in the full run while cycles increase by about 17%.

The disassembly comparison finds the same 1,348-instruction tape-builder sequence apart from relocated addresses and cold panic metadata; the 284-instruction decimal parser is identical after address normalization. This points toward a layout/microarchitecture effect, but does not prove its mechanism. No performance exemption is claimed for unchanged source. The tape builder still validates strings and numbers byte by byte; reducing that work is the next concrete parse target.

For long-ASCII stringify, 955 of 2,245 main-thread samples are under output string allocation; 831 descend into full collection. The direct output copy is therefore not the whole remaining cost. This instrumented workload uses 60,000 calls and cannot replace the shorter matched performance trials. Collector and parse-boundary sources have not been changed.
