# Iterator Helpers

Perry supports the synchronous iterator helpers on `Iterator.prototype`,
including lazy transforms (`map`, `filter`, `take`, `drop`, and `flatMap`) and
terminal operations such as `reduce`, `toArray`, `some`, and `find`.

`join(separator)` consumes an iterator and combines its values into a string.
The separator defaults to a comma; `null` and `undefined` values contribute an
empty string while still occupying a position between separators.

`includes(searchElement, skippedElements)` consumes values until it finds a
SameValueZero match or reaches the end. `skippedElements` defaults to zero and
must be an integer from zero through `Number.MAX_SAFE_INTEGER`, or positive
infinity.

These methods are an additive iterator API slice. Their availability does not
imply complete Node.js 27 compatibility.
