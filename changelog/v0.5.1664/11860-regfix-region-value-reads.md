A loop region whose body calls out no longer takes an array the loop only reads
element values from (`const o = xs[i & 63]; o.m()`). Such an array gains one
cheaper load per read, and the call re-checks the region's guard on every
iteration, so the versioned loop was slower than the plain one: #10594
`userplain` 167 -> 149 and #10510 `date_getTime` 176 -> 158 instructions per
operation. A call-free body keeps these arrays.
