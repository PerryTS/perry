### Use canonical layout metadata in runtime class-field guards

Raw-number class-field guards now read the object's canonical typed-layout
header bit after proving its exact class, keys, and slot bounds. This removes
the remaining per-access thread-local descriptor-map lookup when the inline
guard is disabled, while `PERRY_VERIFY_TYPED_INTACT` retains the independent
descriptor check for GC-layout verification.
