`Object.getPrototypeOf` of a class, a static `super` parent lookup and a
static `super[k] = v` no longer create a class function object for a builtin
parent id (`class E extends Error`): the builtin constructor is the class's
dynamic parent value, and a class without one is a root.
