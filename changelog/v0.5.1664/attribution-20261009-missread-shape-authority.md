Reuse read-site holder entries for function and class constructor own-property
bags, guarding the current receiver's bag shape and loading its inline or spill
slot. Sites no longer rediscover `.prototype` by name on every read of a
FunctionDictionary receiver; entries retain neither the function nor its bag.

Ordinary prototype walks consume the live shape's serial, null or resolved
realm-default link, removing repeated class and anonymous-shape classification.
Declared-class precedence over a colliding anonymous id is projected when the
shape is minted, preserving the reflective semantics under that authority.
The callback-free data walk also preserves its first own-absence proof instead
of searching the receiver's keys again. Getter, Proxy, class-implied and exotic
semantics keep their existing guarded resolution.

Add Node differential coverage for mutation after warm-up, shadowing, deletes,
function bag replacement, getters, Proxy chains, reflected prototype changes,
weak collections and typed-array prototypes, plus mechanism witnesses and a
generic rotating-shape micro-case. The lane scripts reproduce interleaved
instructions/RSS measurements and per-site external uprobe counts.
