On the web target an arrow function reads the `this` of the function that
created it: an arrow class field (`add = (c) => { this.children = c }`), an
arrow nested in another arrow, and an arrow returned from a method now see
their instance instead of an unrelated value.
