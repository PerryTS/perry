- codegen: a class-field site reads or writes a slot as a raw double exactly
  when the slot is an F64 lane of the birth rep of every id its guard compares
  against; a subclass without that lane makes the site boxed (charter step 5, P4).
