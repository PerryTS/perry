`{ ...src, k: v }` of an object with own getters (a bundler's ES module
namespace) copies by the source's layout: keys and attributes come from its
shared key list while its layout word is unchanged, getters are called from
the slot, and the result is born with its keys; once a getter moves the layout
word the remaining keys are copied by name as before. A class object is sized
for the keys its static initializers and static blocks store on `this`, so
`new` of such a class takes its template path. Spread tail keys get room at
birth, a spilled spread source is copied as one block, and a prefix spread
records target positions instead of searching for each key. `this` in a static
field initializer of a class expression evaluated in a function is now that
evaluation's class object, not the shared template (`static #e = this.debug =
…` lost `debug`).
