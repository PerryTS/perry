Made a compiled `C.m()` static call cheaper: the check that the class still
holds the declared method is now one shape-word read and compare per class
the call reads (a direct call costs 6 instructions more than an unguarded
call, down from 17).
