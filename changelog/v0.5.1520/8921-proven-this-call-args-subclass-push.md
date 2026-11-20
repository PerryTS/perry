Compiler and Array performance: a method that passes a declared field's value
to a sibling method (`this.m(this.items[i])`) keeps its proven-receiver clone
instead of re-proving `this` at every site of its public body; and the
generic and spec Array push entries append to an object-backed Array
subclass through its dense fast path before consulting the tracked
allocator resolver. wolf-ecs (noctjs/ecs-benchmark) on the Mac mini
reference box, 2-second window: add/remove -11.2%, entity-cycle -11.9%,
11/11 paired wins each; semantics probes byte-identical to Node.
