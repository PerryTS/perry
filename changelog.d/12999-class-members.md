fix(codegen): visit class bodies during structural region splitting

Offer constructors, methods, accessors, computed members and field initializer
closures to the existing splitter. Preserve frame-operation refusals and shared
constructor captures, and include field-only closure bindings when allocating
region local IDs.
