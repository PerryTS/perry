The exotic-receiver classifier (`Date`, `Error`, `Temporal`, `Promise`, `Map`, `Set` own
properties) decodes a value by its tag alone. A word with clear top 16 bits was kept
as an address and classified by the header found there, so the Number whose bits equal
the address of a live `Map` was taken for that `Map`. Such a word is a Number. The one
caller that passes a typed receiver pointer (`js_object_set_field_by_name`) has its own
entry for the pointer with or without its tag.
