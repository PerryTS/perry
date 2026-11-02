A by-name overwrite of an existing key no longer throws away the object's typed
layout. `try_existing_own_data_overwrite` declared the whole layout unknown
before every in-bounds store, which cleared `GC_OBJ_TYPED_LAYOUT_INTACT` on the
first `o.k = v` a generic store site sent through the runtime; from then on
every class-field read guard on that object missed. The per-slot layout note
the store already runs decides what the value changes (a pointer into an
unmasked slot, a non-number into a raw-f64 slot), as it does for the emitted
store hit. Method call through a parameter or module-`const` receiver whose
body reads `this.a`: 1,376 -> ~200 instructions per call (literal), 1,549 ->
134 (class field initializer), 2,458 -> 135 (constructor-assigned fields).
