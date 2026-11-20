### Diagnostics

`PERRY_REGEX_DIAG` now reports hit and miss counts for the RegExp proofs
(`regex_canonical` exec, method and split, the `RegExp.prototype.test` site
check, and the flag-accessor cache) on a new `[regex-proofs]` line. A run
without the variable pays nothing. No behaviour change.
