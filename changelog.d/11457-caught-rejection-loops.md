Allow async functions to handle arbitrarily long sequences of rejected awaits.
The async-step driver previously synthesized a TypeError after 10,000 consecutive
error resumptions, incorrectly treating caught rejections as a stuck state
machine. A catch block can advance its loop and immediately await another
rejected promise, so the error count cannot establish lack of progress.

Remove that counter and its GC inventory entry. Add a runtime regression that
finishes 12,051 consecutive error resumptions and a native parity fixture covering
numeric, object, and Error rejection reasons. This fixes rate-limiter-flexible's
normal rejection-based API in long-running async loops.
