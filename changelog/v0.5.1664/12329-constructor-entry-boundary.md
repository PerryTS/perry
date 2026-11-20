Constructor-entry optimization boundary coverage (#12329): add Node-parity cases
for first-store key order, escaped aliases, a helper that installs a prototype
setter without receiving `this`, receiver calls, conditional fields, `super`,
and a proxy prototype.

No constructor optimization is enabled by this fragment. After #12327, inferred
constructor fields reserve slots at birth but become keys only at their first
store. Those stores transition the shape and require the existing store-entry
and prototype-chain proof; one birth-shape check cannot replace that sequence.
The runtime store-entry work remains with the tscstore2 owner.
