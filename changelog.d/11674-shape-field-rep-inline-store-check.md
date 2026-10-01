Every compiled fast path that stores into an object's field now respects the
field's recorded representation: when a shape says a slot holds a Number, the
inline store and the inline property-add accept only a plain double there and
send anything else (an object, a string, an integer box, NaN, Infinity) to the
runtime, which re-describes the field before storing. Deleting a property
drops the representation before it moves values between slots. A new
`field-rep-assert` runtime feature (always on in debug builds) checks at every
collection that each such slot holds a double.
