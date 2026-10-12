fix(codegen): preserve computed super assignment order

Computed super assignments retain the raw key until PutValue so the right-hand side, live home-parent lookup, and key coercion occur in JavaScript order.
