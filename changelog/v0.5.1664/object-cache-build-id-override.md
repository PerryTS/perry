### Build

- `PERRY_OBJECT_CACHE_BUILD_ID=<hex>` pins the build-id component of the
  per-module object-cache key, so a `perry` built from a runtime-only branch
  can reuse the objects a sibling build cached under the same HIR and options
  and go straight to the link. Codegen changes still miss through the HIR and
  option fields of the key; an unparsable value is ignored.
