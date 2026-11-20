Guarded direct method calls now propagate exact class and shape facts into
eligible object arguments, including unannotated JavaScript parameters with a
unique declared-field signature. The internal tagged-ABI clones use direct
field offsets while exact runtime guards, shadow roots, and the ordinary method
fallback preserve behavior for subclasses, proxies, mutated shapes, and other
dynamic values.
