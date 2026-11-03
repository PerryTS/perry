Fixed a class-method call site's learned-receiver arm bouncing through the
method's public JSValue wrapper when the method has a parameter-typed clone
(typed f64, i32, boolean or string parameters). A receiver of the declared
class whose shape differs from its birth shape now joins the declared-class
arm: the arguments are guarded inline and the typed clone or the internal
generic body is called directly, as for a birth-shape receiver.
