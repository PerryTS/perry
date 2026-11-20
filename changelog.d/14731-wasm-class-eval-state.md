On the web target, static fields and `static {}` blocks run once, in source
order, with `this` bound to the class. A class declared in a function body
evaluates its computed member names and installs those methods, accessors and
fields (not yet a Symbol-keyed accessor), with fresh statics per evaluation.
