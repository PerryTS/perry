Fixed a ~1,300 instructions-per-store regression on `Object.create(proto)`
receivers. Since `Object.create` stopped minting a synthetic class id per call
(#11166), its result is class-less, and nothing marked it an ordinary object, so
the static-key store site's receiver-kind test refused it: every `o.k = v`
missed the site cache and took the full `[[Set]]` walk, even for an own data
property. `Object.create` now births its result ordinary, like the other
ordinary birth sites, and the store site publishes its shape as it does for a
literal or a class instance. On the acceptance matrix the `Object.create`
overwrite cell goes from 1,588 back to about 300 instructions per store.
