Compiled x86-64 Linux executables are smaller. Tag tests and masks on JS values
used to encode their 64-bit NaN-box constants as a 10-byte `movabs` at almost
every use. The runtime now holds these constants in one read-only table, and
compiled code reads them from it as memory operands, which the processor folds
into the compare, `and` or `or` that uses them.
