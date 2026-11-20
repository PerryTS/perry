### Faster

- `String.prototype.replace` with a function replacement spends about 27% fewer instructions, and roughly a quarter less on non-ASCII subjects. Each match used to build a JS array to carry the replacer's arguments, growing it one push at a time and then reading it straight back out into the native slots the call actually uses; the arguments now go directly into those slots, which were already the collector's roots for them. A proxy replacer, whose `apply` trap really does receive an arguments array, is unaffected (#10165).
