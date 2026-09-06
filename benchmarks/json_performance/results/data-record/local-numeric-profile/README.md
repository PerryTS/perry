Busy local development-host profiles; not qualified CPU/RSS measurements.
Both workers and samplers exited successfully. Commands/hashes are recorded.
The same numeric fixture is parsed 4096 times with two warmups and default GC.

Tape construction dominates both samples: build_tape_into has 1465 of 2199
main-thread top-of-stack samples for Unicode and 1455 of 2182 for data-record.
Depth preflight contributes 227 and 182 respectively. This narrows the next
investigation toward tape construction and scanner code generation, without
establishing a precise CPU split or cause of the measured regression.

The ARM scanner currently stores its comparison mask and searches the 16 bytes.
An independent leaf experiment replaces that hit handling with two register
word extracts and trailing-zero counts. It reduces emitted scanner code while
keeping the no-hit loop unchanged; whole-runtime performance remains to test.
