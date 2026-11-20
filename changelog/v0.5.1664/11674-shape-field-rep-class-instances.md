Class instances can now keep a Number-only field in its unboxed form too. The
restriction that kept every class instance on the boxed representation is
gone: every class-field fast path already matches the instance by its exact
shape, and a shape that marks a field Number-only sends other values through
the checked path.
