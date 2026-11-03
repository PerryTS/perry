Whole-program facts no longer report a false E14 for an element key of top:
such a key never names the constructor, so a read through it keeps its
other proofs. A call through a parameter whose call is never run produces no
result value, so nothing flows from it into later reads. Per-unit proven
read/call sites rise on the chess witness (477 to 510 of 667) and its tests
(594 to 643 of 821); whole-program counts and the large ledgers are unchanged.
