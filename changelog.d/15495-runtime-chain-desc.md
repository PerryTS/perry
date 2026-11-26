Inherited, absent and `in` property reads (and for-in over a prototype
chain) resolve through a per-prototype immutable chain descriptor
(ChainDesc) held in the prototype's owned-state bag: one masked-hash probe
instead of walking the chain. The descriptor is rebuilt eagerly when the
chain changes (mark transition, add/delete on an ancestor, proto relink);
reads stay plain, with no site tables, caches or epochs. Absent-key reads
drop up to ~40% of their instructions (`class` receivers −40%,
`Object.create` −37%, literal/JSON/dynamic −17%).
