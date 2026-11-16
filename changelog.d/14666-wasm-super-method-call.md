On the web target `super.m()` runs the parent class's method, from a class
method, getter, setter or constructor or an arrow inside one, including through
a class that does not define `m` itself, instead of returning `undefined`; an
ancestor's `get m` shadows a further ancestor's method `m`.
