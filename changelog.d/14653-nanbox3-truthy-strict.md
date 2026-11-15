`js_is_truthy` no longer reads a Number as a string. Its legacy arm took an
8-byte-aligned word with clear top 16 bits above the handle band as a raw
string pointer and read its length: an address-shaped Number (a denormal
double) crashed with SIGSEGV (`stream.getDefaultHighWaterMark(e)`), and a
Number whose bits were an empty string's address was falsy. Such a word is a
nonzero Number now, so it is truthy, as in Node.
