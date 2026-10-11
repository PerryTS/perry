Sync generators inherit `next`, `return`, and `throw` from the intrinsic
generator prototype and own one private resume record. Its captures hold the
generator controls and body locals crossing resume states in fixed slots;
locals confined to one state stay on the stack, and bindings visible to
user-created closures retain their shared cells. Abrupt resumptions share
one state dispatcher. Instances are born with their final prototype and brand,
removing own protocol closures, prototype transitions, and const-function
finalization from generator construction. Compiler-owned scalar controls
retain native operations in those slots. The existing method memo unifies
empty generator chains under the shared intrinsic holder, while checking
live prototype and callable identities per use. Generator function methods
reuse the existing entry-owned prototype-chain proof, including their first
intrinsic holder, so inherited `apply` calls can reuse their actual callable.
Async generators retain their existing queue and state lowering.
Fixed records use the collector's existing JSValue tag scan and exact slot
barriers. The synchronous protocol proof ends at its own holder and requires
no Object parent bootstrap.

Protocol coverage includes completion, abrupt resumes through yielding
finally blocks, delegation, methods, escaped closures, property additions,
and replacement of the intrinsic `next` method.

The shared runtime boundary owns completion, execution guards, default abrupt
resumes, sent-value publication, exception setup, and result allocation.
Private bodies keep one integer switch over their source states. Fixed-slot
stores call one shared barrier operation, including binding string-alias
demotion; construction-proven scalar writes remain one store. Generic reads
in continuations use the existing complete property-read helper and its
existing cache, removing repeated receiver, accessor, and miss ladders.
Native argument-vector root save/restore loops are also emitted once.
The method memo's existing live proof selects the intrinsic's direct rooted
resume edge without validating the generator protocol a second time.

The genmodel2 evidence report records the current-main n=5 interleaved
instruction, RSS/full-collection, and emitted-text comparisons, together with
44 Node parity gaps, mutation controls, protected copying collection, and
all four class micros. No new cache, latch, or side table is introduced.
