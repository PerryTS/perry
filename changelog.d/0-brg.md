### Fixed

- Restrict family-method probes to layouts whose static method index contains the requested name, avoiding guaranteed misses for unrelated class families.
- Mix fresh symbol identities once at creation so small property tables distribute symbol and string keys without depending on how many symbols startup created. Preserve symbol identity, enumeration order, and move stability.
