Move per-instance builtin non-constructability into the closure's GC header. Delete the address-keyed set, young log, root scanner, death prune, and rekey verdict. Existing length metadata remains in the closure property bag. The closure and capture ABI do not grow.

Add a copying-GC test with a flag-removal negative control and a JavaScript parity gap covering a bound builtin and an ordinary constructor.
