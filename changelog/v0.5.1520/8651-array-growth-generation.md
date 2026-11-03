Fixed retained array-growth forwarding stubs pointing into resetting nursery
space. When an old array outgrew its backing storage, `js_array_grow` could
allocate the replacement in the copying nursery and leave the permanent old
stub pointing at it. Minor GC does not trace that forwarding payload as a
normal array slot, so a later nursery reset could recycle the target while
stale aliases still followed the old forwarding word.

Array growth now keeps the replacement out of the nursery whenever the source
is old or otherwise non-moving. A young source uses nursery space only through
the no-collection allocator; if growth could collect and promote the source,
the replacement is born old instead. This fixes the intermittent ECS failure
reported as `Cannot assign to read only property 'length' of object`.
