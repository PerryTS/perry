- codegen/runtime: a nominal class parameter guard (`OP_CLASS_NOMINAL`) now carries
  the class chain's field count and proves its value half from the receiver's
  shape (an `F64` lane on every field slot) instead of the typed-layout-intact
  header bit; codegen no longer bakes that bit into inline allocation images, and
  the conforming layout-note skip tests the SIDE_MASK layout state alone
  (charter step 5, P4 flip).
