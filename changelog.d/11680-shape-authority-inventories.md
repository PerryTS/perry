Keep the one-shape source checks aligned with the completed migrations. Remove
the four header-offset callsites deleted with the legacy class-loop path, lower
the SSO debt inventory for the removed heap-only key reader, and retire the two
deleted class-guard constant registrations.

The descriptor census now follows ConstFn's wrappers to the shared mint and
checks that the descriptor is published before both reverse indexes. Two
negative controls reverse those orderings and must fail. The original shape
census, header-constant check and its negative control, and SSO inventory pass;
no debt ceiling or runtime performance tolerance is raised.
