The private-member guard moved to its call sites, taking a call off every
ordinary property read and write.

#8970 made the private-member name test cheap but left the CALL. In a pure
property-read loop (`o[k]` with pre-built keys, no concat) that showed up as
`private_member_get_by_name` 11.4% plus `private_member_storage_name` 5.4% —
**16.8% of the loop, the largest single item** — essentially all of it call
overhead for keys that are rejected on their length before doing anything.

The guard is now invoked at the three call sites (the generic read entry, the
class-field read miss, and the generic write), so an ordinary property
operation makes no call into the private-member path at all. Keys that pass
the guard take exactly the original path.

Interleaved A/B, min-of-21 (under heavy co-tenant load, so read the ratios
rather than the absolutes): pure property read 26 → 22 ms (−15%), computed-key
read 51 → 45 ms (−12%), combined overwrite 50 → 46 ms (−8%), write unchanged.

Output on a private-member exercise — instance fields, `static #instances`,
private methods, private getters, `#x in obj`, subclassing, and an ordinary key
literally named `#<perry:private-member:1:x>` — is byte-identical to before the
change. Computed-key differential vs node is byte-identical. Suite 2779 passed.
