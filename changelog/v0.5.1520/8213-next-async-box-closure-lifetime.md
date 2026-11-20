## Fix completed async frames retaining closure-visible box cells

Plain async functions now hand their complete boxed activation frame to the
release/reuse path. Closure capture counts follow GC moves, and authoritative
death pruning drops them. Full collections trace drained box payloads from
their live closures instead of rooting every pending box, so self-referential
box/closure cycles can die. After queued/running steps drain, uncaptured cells
publish immediately while each captured cell remains readable until its own
final closure dies, so one escaped closure cannot retain the rest of the frame.

This removes the closure-visible residue left by #8208 without weakening raw
box-pointer rejection or returning box-cell memory to the allocator.
