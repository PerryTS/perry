Repair two runtime fixtures whose distinct-class premise ceased to imply
distinct layouts after canonical key interning. The numeric-write guard now
sees the same target names in different ordered slots (`a,b,c,d` versus
`a,b,d,c`). The imported typed-shape fixture uses different key names at the
same slot count, independently of its existing slot-count mismatch. Both
assert distinct key arrays and retain their rejection assertions.

Validation uses release builds with `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`,
the 24 GiB suite cap, and `--test-threads=1`. Each repaired test passes in its
own process. Disabling the production uniform-ShapeId check or the imported
keys-validation check makes the corresponding repaired test fail on its own
rejection assertion; both sabotages are reverted.

The delete-lane seam remains deliberately unconverted. A temporary probe of
`tombstone_hole_count_survives_readd_append` observed the re-add replacing an
owned keys array with a different, shared canonical array and changing the
ShapeId, while retaining one hole. Its unchanged hole-accounting and churn
bound assertions passed: 60 churn cycles ended with 20 stored slots. The
original stable-identity assertion remains intact and failing. Resolving the
owned delete/re-add contract requires the deferred delete-lane conversion;
no special-case bypass of canonicalization is added. The requested stop at
that boundary leaves the tsc A/B unrun; no performance result is claimed.
