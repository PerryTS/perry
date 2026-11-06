Whole-program facts read a computed key held by a `const` binding with a
string-literal initializer (`const k = "a"; o[k]`) as the named key, as they
already did for constant stores and definitions: the read sees only that
field, and the element site's key fact names it. A binding that may be
reassigned, or a `const` of a non-literal value, still names any key.
