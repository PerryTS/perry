Closed World receiver proofs now cover object-literal classes. A literal with
no methods compiles to an `__AnonShape_*` class and is born with that class
id in its header, and the binding facts already listed those ids, but the
driver handed codegen only the facts of `kind=class` templates. Every
prefix-stable template with a class id now reaches the zero-guard tier and
the typed-body (5b) receiver proofs; a binding that can also hold ⊤ gets a
checked proof (class id compare, generic read on a miss), as decision 150
requires. tsc goes from 0 to 686 elided reads (478 checked, 208 guarded),
effect from 0 to 217, fastify from 0 to 140; output and PERRY_PROBE_VERIFY
unchanged.

Function instances (`new f()` on a constructor function) still get no proof:
they are born with class id 0, so there is no id to check until constructor
functions get a birth family.

`test-files/wpfacts/l00`–`l03` cover literal reads (unchecked and checked
against interpreted code), literals with methods, and function instances
with prototype-assigned methods; `scripts/wpfacts_corpus.py` gains an
`elide .key <proof>` assertion read from the `.elisions` dump.
