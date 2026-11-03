Fix class method calls ignoring `Object.setPrototypeOf` on an instance (#12466).
On a canonical ShapeId miss with no own method, compare the live prototype
edge encoded in the header: a nominal edge retains the existing guarded class
tower, while a foreign or null edge uses the existing generic method operation.
No runtime API or cache is added. Regression coverage includes in-place
prototype method replacement after the first call, subclass prototype edges,
Object.create overrides, own/prototype mutations, inheritance, and `super`.
