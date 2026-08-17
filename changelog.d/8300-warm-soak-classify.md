### Fixed — the warm-soak arm reported a client timeout as an #8163 residual

`run_warm_soak` (added in #8215) counted non-zero `verify.mjs` exits instead of
classifying them, and printed `#8163 residual` for every one. Two very different
things produce that exit: the residual, where the server returns an **empty body**
(`Unexpected end of JSON input`, plus `TypeError: value is not a function` and
Next's `E180` in the host log), and a **client timeout** (`UND_ERR_HEADERS_TIMEOUT`),
where the server was merely slow and its log is clean.

The second happens on its own in the saturated regime the arm exists to reach — in
the #8163 close-condition run, 12 passes lost requests to client timeouts, all
inside collection storms (one pass ran **644** copying minors against a steady 84),
while the three residual discriminators were **0 across 10,995 collections**. So the
arm manufactured false residual reports exactly where it was most likely to be used,
and #8163 is closed, so the next reader would have concluded it had regressed.

It now classifies from the evidence already in the host log: an empty body fails the
arm as the residual; a timeout with a clean host log is reported loudly with the
offending pass's collection count and attributed to #8213 (a warm server slow enough
to blow a default HTTP client timeout is a user-visible failure, but it is not this
arm's subject, and failing on it would make the arm unusable in the regime it exists
to reach); anything unrecognised fails with "do not assume a cause".

The arm was written so a *green* run could not imply more than it proved. Its first
real failure was the mirror image — a *red* verdict that was not true. A gate that
cannot be trusted when it goes red is as useless as one that cannot go red at all.
