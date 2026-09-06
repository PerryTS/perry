Disassembly of js_json_parse in the qualified empty-object and primitive-object
workers, identified by their complete hashes and objdump commands in summary.json.
Both bodies contain 372 instruction addresses and reserve a 0x90-byte frame.
Even input beginning with an object/array delimiter executes the initial
register saves and matching restores before tail-calling parse_slow. The scalar
body is inlined into the public entry and brings this frame to the common
container branch.

This is static code evidence of avoidable entry work, not a CPU speed estimate.
Equal instruction counts do not prove equal instructions or execution cost.
The two function starts also have different alignment; the disassembly does
not prove that alignment causes the measured scalar differences. A future
experiment can outline scalar decoding after the early container branch,
while preserving the exact pending-collection and rooted slow-path semantics.
