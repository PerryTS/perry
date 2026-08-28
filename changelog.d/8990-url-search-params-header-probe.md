`url_search_params_backing_of` decides from the GC header instead of a by-name field lookup.

The probe previously opened a handle scope, allocated a heap string for the backing key and ran a full by-name lookup for *any* pointer-tagged receiver — so every `Map`, `Set`, array, closure and Date cell paid all three only to be told `None`. Only an ordinary object can carry a named field, and the header already says so.
