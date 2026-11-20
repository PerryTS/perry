Whole-program facts: a class whose parent is a built-in constructor
(`class HttpError extends Error`) no longer hands its instances to unknown
code. `super()` runs the value the `extends` clause evaluated; the solver now
checks that value while it solves: when it is the constructor the global
object declares under the parent's name, `super()` is the native constructor
summary and the instance reaches no program code. Any other value (the global
replaced by the program, an unknown value from an unseen module, `null`, a
local binding of the same name) escapes with the instance as before. In
three.js this was the first link of the chain that hands every math class to
unknown code (`class HttpError extends Error`).
