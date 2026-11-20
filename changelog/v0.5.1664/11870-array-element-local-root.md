A local initialized from an array element is shadow-rooted again. The pointer
analysis took the element type from the array literal (`[0]`, `[undefined]`)
and missed in-place element writes, so `const a = [0]; a[0] = obj; const x =
a[0]` left `x` holding `obj` in a plain stack slot across collections. HIR
builds that shape for a repeatable class declaration whose members name the
class, so a factory's class object went stale under a moving minor (#11862).
