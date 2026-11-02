Exercise generated WASI 0.3 resource-bearing futures with repeated labels for
the same imported interface, including wrong-owner rejection, independent
native callbacks, ownership recovery, exactly-once drops and Store reuse.
Emit plain 0.3.0 extern names after composition resolves the labeled nominal
types, avoiding a runtime dependency on the 0.3.1 implements annotation gate.
