Class method values are read from the shape. Reading a declared public
method as a value on a receiver whose class the compiler knows (`this.parse`
in Zod's `this.parse = this.parse.bind(this)`, `const f = obj.m`,
`arr.map(obj.m)`) called `js_class_method_bind_by_id` on every read. That
helper turned the method id back into a name, walked the class chain by name
to find the method's owner, looked for an own property of that name, and then
looked the canonical value up in name-keyed tables. The read now uses the
ordinary property-read site: one ShapeId compare and a load of the slot on a
hit, and the site's holder entry for an inherited method. An own property
that shadows the method, an accessor, a prototype replaced or edited after
the read, and `setPrototypeOf` are all seen by the shapes the read already
checks. Private methods and native handle methods keep their own routes.

Zod ×5000: −34.6% instructions (12.52 G → 8.19 G), RSS unchanged, output
identical to Node. An inherited method-value read on a parameter receiver
costs 183 instructions instead of 1,627 (Node: 20). The rest of that cost is
the out-of-line holder answer; see the open questions in #12016.

Tests: `test_gap_method_value_shape` (identity within and across instances,
bound versus unbound, getter-made methods, own shadowing including
`undefined` and accessors, constructor self-binding over subclasses,
prototype replacement, delete and `setPrototypeOf` after a read, prototype
accessors, `super` method values; matches Node). The codegen test
`method_value_shape_tests` asserts that the read is a shape site with no
class/name call. Restoring the class/name route turns it red.
