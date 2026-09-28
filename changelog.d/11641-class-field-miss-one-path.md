A class-field read that its inline guard cannot prove now takes the One Path
read instead of a by-name walk. `this.x` inside a class or object-literal
method compiles to a guarded slot load against the class's birth shape; a
receiver the guard did not describe (an `Object.create` child of the literal
calling its method, an instance grown past the birth shape, a subclass the
guard does not name) went through `js_object_get_field_by_name_f64` on every
read, with no site memo. The miss arm now carries the site's own read cache
and asks the receiver's shape first (the site's word, then the inherited-read
cache), then the generic read ladder, exactly as `o.x` does at any other site.
Inherited read through `this` on an `Object.create` receiver: 1,318 -> ~290
instructions per read (the same read through a parameter is 311). Typed
feedback builds keep the guard and its recording unchanged.
