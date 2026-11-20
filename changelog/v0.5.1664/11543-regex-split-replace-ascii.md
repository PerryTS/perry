### Faster

- `String.prototype.split` with a regular expression and string-template `String.prototype.replace` build their pieces with far less host work on ASCII strings (#10165, #10518):
  - A split piece or a replace capture is now one byte copy. It used to take two unit-by-unit passes and two GC safepoint polls.
  - A replacement's output is now one allocation plus a copy per piece. It used to decode and re-encode every unit twice.
  - The split loop polls once per 512 units rather than once per piece.
  - A split with no capture groups no longer builds a capture array for every piece.
  - A split's private output list appends without a catch frame while it has spare capacity.
- Measured on qb2 in instructions: `encodeURI(s).split(/%..|./)` −43%, a short global replace −15%. validator's `sanitize` and `batch` package workloads are −4.2% and −3.3%. Peak RSS is unchanged within 1%.
