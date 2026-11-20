Refresh HIR unit fixtures for mutable Map construction, retained Set literal lowering, array-iteration scan facts, and lexical blocks that retain their own HIR scope. Annotate Map/Set receivers explicitly in collection-view fast-path witnesses so their type proof remains meaningful. Keep the shadowed binding and early-read checks.

These changes update tests to the current lowering; namespace lowering is untouched.

Check captured cells structurally after the Array allocation-hint addition. Mixed spread pushes retain one live method lookup and their original argument order.
