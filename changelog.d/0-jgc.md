Allow a pending parent-mortality probe to promote young blocks in place after its current trace meets the existing survival criterion, retaining evacuation rollback and GC stress exclusions.
Reuse exact promotion-walk census facts in an immediate full collection for traced promotions as well as untraced promotions.
Inline the existing function-name read adapters so metadata queries avoid wrapper call/return and Option filtering overhead.
Use the actual ASCII bytes to validate metadata names directly, preserving checked decoding for Unicode and invalid input.
Refresh adopted header facts after minor mark clearing so traced census reuse preserves whole-block reclamation.
