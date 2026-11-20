Fixed named class expressions whose static methods update a static private
field through the class's inner name from inside a nested arrow. The write side
of `c.#field++` now preserves the same lexical class-evaluation brand as the
read side instead of falling back to the arrow's absent `this` binding.
