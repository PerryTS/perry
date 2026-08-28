### Changed

- Appending to and popping from a `class X extends Array` instance no longer re-classifies its own elements store: an in-capacity append and a non-hole tail pop run without a handle scope, a proxy probe, forwarding-stub cleaning or flag resolution, keeping only the element bookkeeping the read tiers and the loop guard consume. Growth, holes, an empty store and every exotic flag keep the complete runtime entry.
