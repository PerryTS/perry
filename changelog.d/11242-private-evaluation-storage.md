Fix repeated evaluations of one class sharing instance-private storage (#11163).

Each evaluated class now has a move-stable scalar storage identity. Instance-private values, field markers, and method/accessor brands use that identity, allowing one evaluation to extend another and initialize both sets of private elements on the same object. Brand checks search pinned instance heritage by evaluation identity; static brands still require the exact constructor.

Constructor replay supplies its own evaluation as the lexical private owner. Private-access guards retain their selected owner through field reads/writes and accessor or method dispatch, including direct static calls whose receiver argument belongs to a derived evaluation. Extracted private methods retain their lexical owner. Pending owners are traced and rewritten by the existing private-brand root scanner, replacing their former pointer-free inventory exemption.

Regression coverage includes repeated-template inheritance, instance/static reads, private methods/accessors and extracted methods, unrelated-brand rejection, nested calls, and actual copied-minor relocation of both the storage identity and a pending access owner.

Dynamic `super` calls now resolve the lexical class evaluation's pinned parent and carry that parent's private brand through instance, static, and spread dispatch. This avoids the template parent table's rejected same-ID edge and prevents a parent method from reading a derived evaluation's private storage.

Generic PutValue writes resolve private storage before the ordinary own-field overwrite fast path. Preallocated template slots no longer intercept a guarded write or leave its private-owner hint behind. Tests cover the hinted fast-path case, repeated parent evaluations, and direct, compound, update, and destructuring private writes.
