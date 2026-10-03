Method dispatch's own-key shadowing check compares a receiver of at most eight
own keys directly again instead of hashing the name and probing the shared key
index: on a one-field class instance the index cost about 270 instructions per
call against about 25 for the compare. Wider receivers keep the index. Part of
the #10507 `decimal_class` regression.
