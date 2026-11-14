The brand checks of the reflective `Set.prototype.*` / `Map.prototype.*`
methods (`set_ptr_from_receiver_bits`, `map_ptr_from_receiver_bits`) decode the
receiver by its tag alone, and the receiver handed on to `forEach` / `add` /
`set` is no longer re-tagged. A receiver with clear top 16 bits was a candidate
Set / Map address (registry-guarded): a Number whose bits equal a live
collection's address was taken for that collection. Such a Number is now a
primitive receiver (TypeError), like every other Number.
