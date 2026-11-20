Added a field-representation section to the heap census (`PERRY_GC_CENSUS`):
for every live shape it reports which inline slots hold a Number in every
object carrying the shape, how those Numbers are boxed, which slots mix Numbers
with other values, the fork multiplier a per-slot representation in shape
identity would cost, and the objects' share of the per-object layout tables.
Diagnostic only; nothing changes when the census is off.
