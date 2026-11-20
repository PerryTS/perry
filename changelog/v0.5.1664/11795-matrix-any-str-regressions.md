Fixed the loop-region slowdowns #11680 introduced for fields that are not
Number fields. A Number read of an `any` field (or of a field born a string)
no longer refuses the whole region: the guard tests the slot's value on the
object instead (`h += o.a` on an `any` class field: 71 -> 11 instructions per
iteration). A string read through `.length` no longer asks for a Number lane
(92 -> 64), and a method call on a receiver whose class is proven keeps its
guard-free direct call inside a region (90 -> 61).
