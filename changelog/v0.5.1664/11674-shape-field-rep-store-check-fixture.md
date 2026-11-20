The field-representation store-check test now reaches the compiled store and
property-add fast paths with a non-Number after they were primed on a Number
field, and forces a collection after each case, so a fast path that skipped
the check fails the test instead of passing unnoticed.
