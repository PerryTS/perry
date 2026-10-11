Fix replay of recorded ordinary birth shapes after literal allocation acquired
its marked ordinary kind. The replay allocator now derives the newborn's kind
from its shape and validates that shape again after allocation. This restores
EventEmitter's existing birth mechanism without adding a cache or alternate
constructor algorithm.
