### Fix Object.assign dropping ordinary source keys (#11965)

Native namespace enumeration interpreted an arbitrary object's first data slot
as a module name. On Object.assign's descriptor-driven path, an options object
beginning with `name: "console"` therefore enumerated console exports and copied
only its overlapping `error` key. The response Set's contents were irrelevant.

Record native namespace identity in the existing shape kind and require that
kind before enumerating virtual exports. Ordinary objects and runtime-built
property descriptors now enumerate their own shape keys; genuine namespaces
retain their export surface. Inherited setters and read-only descriptors keep
Object.assign's strict Set behavior from #11897.

The dependency-free regression exercises the 11-key endpoint layout, empty and
non-empty Sets, direct own-key reflection, runtime-built descriptors, property
order, another module name, and genuine namespace enumeration.

On the tested base (4c87719783), the full Effect beta.83 repro stops before
endpoint output with `Bind must be called on a function`, both before and after
this fix. Its end-to-end output remains unverified; the reduced regression
reproduces `1:undefined` on main and matches Node's `11:console` with the fix.
