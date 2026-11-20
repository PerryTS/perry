`JSON.parse` no longer crashes the process on deeply nested input.

A document nested a few tens of thousands of levels deep did not throw — it
killed the process with SIGSEGV and printed nothing at all, so a program had
no way to see what happened, let alone recover:

```
node    → parses it
scriptc → throws a catchable RangeError
perry   → SIGSEGV, exit 139, no output
```

Deeply nested JSON is a well-known shape for untrusted input, which is what
makes a crash the wrong answer even though such a document is unusual.

Two parsers read the text, and both descend one function call per nesting
level: the syntax-validation pass and Perry's own value parser. Deep enough
input exhausts the stack in whichever reaches it first.

`JSON.parse` now measures nesting depth first and throws a catchable
`RangeError` when it exceeds 1,000 levels. The measurement is a single
non-recursive scan of the text, which matters more than it sounds: a recursive
depth check would crash on exactly the documents it exists to reject. It also
runs *before* syntax validation, since that pass recurses too and would crash
first otherwise — which means the scan sees malformed input and has to cope
with it, so brackets inside strings do not count and a stray closing bracket
clamps at zero instead of underflowing.

The limit is 1,000 because it has to be safe on the *smallest* stack in the
process, not the largest. Perry parses JSON on worker threads as well as the
main thread, and a 2 MiB thread stack overflows far earlier than the main
thread's 8 MB does. A first attempt used 10,000, taken from a main-thread
measurement, and the unit test crashed the test harness at 9,999 levels — so
the number is what a small stack can carry, not what a big one can. It matches
the depth Python's parser settled on, and real documents are not close: JSON
nested past a hundred levels is already unusual.

This is a deliberate gap against Node, which parses far deeper because V8's
parser is iterative and consumes no stack per level. Closing it means making
Perry's parser iterative too, tracked separately. Until then a catchable error
is strictly better than a crash.

Refs #7792.
