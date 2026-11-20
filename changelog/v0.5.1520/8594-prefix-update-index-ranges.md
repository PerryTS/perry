Eliminated dynamic typed-array property lookups for bounded update-expression
indices such as `P[++i]`. Integer range analysis now models the distinct prefix
and postfix results and conservatively retains loop facts across updates. On
the Blowfish-shaped `typed_array` workload this reduced retired instructions
from 55.717 G to 14.647 G (-73.71%) and wall time from 4.063 s to 2.498 s
(-38.51%) on the quiet M1 mini, while the typed/untyped ratio stayed at 1.000
and peak RSS decreased 1.19%.
