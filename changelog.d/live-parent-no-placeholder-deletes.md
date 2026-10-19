Stop deleting 41 class-field placeholders per construction of a class whose
parent is only known at runtime. Instances are born with every field slot the
hole (or with an empty key set), so a parent constructor still creates its own
keys first and the derived fields are defined after super() in source order.
The per-field js_object_delete_dynamic_value calls removed here were deleting
keys that were never present. Commander parse_argv: 3.23G to 2.76G instructions.
