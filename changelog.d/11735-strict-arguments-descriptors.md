Strict escaping `arguments` objects now keep their `length` and restricted
`callee` attributes in the shared key layout, with the thrower accessor in the
object's own slot. Construction no longer installs per-object entries in the
address-keyed property and accessor descriptor tables.
