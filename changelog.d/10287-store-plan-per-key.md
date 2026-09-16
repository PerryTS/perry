The store-plan cache is vetted per key rather than per receiver. It exists so a
property store need not re-run the interception vet — the prototype-chain walk,
the class-registry lookups and the `Object.prototype` probe — but it refused any
receiver carrying a property descriptor at all. zod installs `_zod` on every
schema object, so none of them ever held a plan and every one of their stores
paid the full vet again.

Own descriptors disqualified the receiver for a real reason: an own accessor
must dispatch through a short-circuit that a plan hit skips. That is a per-key
fact, and the same path already proves the key uncovered, so the plan is now
denied only for the keys a descriptor can actually cover.

Constructing 300 real zod v4 `z.object` schemas drops a further 7.3%, and a
fixture building 2,000 receivers that define one non-enumerable property and
then assign 40 properties drops 19.7% (1.02 to 0.82 billion instructions). The
same fixture without a descriptor is unchanged.
