Busy local development-host profiles; not qualified CPU/RSS measurements.
Both workers and samplers exited successfully. Commands/hashes are recorded.
The same numeric fixture is parsed 4096 times with two warmups and default GC.

Tape construction dominates both samples: build_tape_into has 1465 of 2199
main-thread top-of-stack samples for Unicode and 1455 of 2182 for data-record.
Depth preflight contributes 227 and 182 respectively. This narrows the next
investigation toward tape construction and scanner code generation, without
establishing a precise CPU split or cause of the measured regression.

The ARM scanner expresses a mask store and byte search in Rust, but later
disassembly showed LLVM already eliminated the store. An independent experiment
replaced the lane search with register-word extracts and trailing-zero counts.
It reduced emitted code but regressed escaped parsing by 50.63% in a qualified
full matrix and was reverted. See ../../neon-mask/README.md. This result keeps
the numeric regression open and rules out that proposed replacement.
