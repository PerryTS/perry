Fix `in` checks for getters and setters declared on classes that capture local
variables (#11112). Recognize virtual ClassBody accessors on each prototype
actually visited by the property-presence walk, including per-evaluation
prototypes, without invoking getters. Deleted accessors and replaced prototype
chains remain authoritative.

Add a runtime regression and native parity coverage for getter-only/setter-only
members, evaluated prototypes, inheritance, deletion, redefinition, and prototype
replacement.
