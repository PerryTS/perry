Make small fixed-layout class and anonymous-object births with complete parameter
constructors write their header, shape and metadata directly after the nursery
bump. Boxed-only constructors write final values there; numeric constructors
retain the existing guarded stores after representation-default fills. Their
idle fast arm skips the register-preserving shared initializer; boxed-only
births also skip the constructor's field checks. Nursery overflow and active
birth colors use the existing shared allocator. Incompatible numeric values
retain the guarded constructor fallback. Other layouts retain shared
initialization to bound code size. Literal layouts newly admitted for constructor
elision require constant field arguments; dynamic boxed arguments retain shared
initialization rather than extending their live ranges at every site.
The complete-store proof also excludes constructor return overrides, so fused
births return the relocated instance directly instead of another collecting check.

Use the allocation's shape contract and complete constructor-store proof to
fill boolean-only anonymous literals in place too, removing the obsolete
GC-mask profitability check that forced them through a standalone constructor.

Keep field representation defaults identical to the minted shape, including
unused floor slots. Native statepoint coverage and negative controls verify
argument relocation on the slow arm and instance relocation in constructors.
