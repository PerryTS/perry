### Fixed

- Restrict family-method probes to layouts whose static method index contains the requested name, avoiding guaranteed misses for unrelated class families.
- Mix fresh symbol identities once at creation to spread their hash bits in mixed symbol/string property tables. Preserve symbol identity, enumeration order, and move stability.
