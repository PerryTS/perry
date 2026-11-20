Add profile-guided function layout. `perry compile --record-function-order`
builds a binary that writes the names of its compiled functions to
`$PERRY_FUNCTION_ORDER_OUT` in the order they first run (module init bodies
and string-literal init chunks included); `perry compile --function-order FILE`
places the listed functions first and together, in list order, so the code a
program runs at startup shares pages instead of faulting in text from all over
the binary. ELF builds put each listed function in a ranked `.text.sorted.*`
section (sorted by GNU ld; lld also gets `--symbol-ordering-file`), Mach-O
builds pass ld64 `-order_file`. Unlisted functions keep their order, unknown
names are ignored, and normal builds carry no instrumentation.
