The runtime store funnel for object fields checks the field representation of
the receiver's shape. A Number stored into an `F64` slot is written as its
canonical double (an INT32 box as its double, any NaN as the canonical NaN);
any other value first generalizes the slot for the whole shape lineage (the
shape learns the slot deprecated, the receiver moves to the normalized shape,
shape word before the value), and objects still carrying the old shape move on
their next miss. Nothing produces an `F64` slot yet, so no program changes.
