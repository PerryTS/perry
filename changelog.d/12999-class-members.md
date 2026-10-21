fix(codegen): size namespace expansions when splitting class bodies

Offer constructors, methods, accessors, computed members and field initializer
closures to the structural splitter. Include the cost of whole namespace values
from the existing dependency HIR when admitting bodies and cutting statements;
qualified member reads keep their direct binding cost. Preserve frame operations,
shared constructor captures and field-only closure local IDs.
