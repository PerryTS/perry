// Gap test: slugify's second argument is a replacement string OR an
// options object ({ replacement, lower, strict, trim }). The options
// object used to be coerced through the string path, garbling
// slugify("Hello World", { lower: true }) into "hello{world".
// All inputs below stay inside the accent/charMap subset the native
// binding supports, and outputs are fully deterministic.

import slugify from "slugify";

// Positional-string form (kept working).
console.log(slugify("Hello World"));
console.log(slugify("Hello World", "_"));

// Options-object forms.
console.log(slugify("Hello World", { lower: true }));
console.log(slugify("Hello World", { replacement: "_" }));
console.log(slugify("Hello World", { replacement: "__", lower: true }));

// npm semantics: default keeps case; '!' is in the keep-set; ',' is not.
console.log(slugify("Crème Brûlée!", { lower: true }));
console.log(slugify("Hello, World! (2024)", { lower: true, strict: true }));

// charMap expansion: '&' -> 'and'.
console.log(slugify("Foo & Bar", { lower: true }));

// trim (default true) + custom replacement.
console.log(slugify("  padded  input  ", { replacement: "_" }));

// '-' and '_' are kept literally; only whitespace runs collapse.
console.log(slugify("a - b"));
console.log(slugify("foo_bar-baz"));
console.log(slugify("UPPER Case Kept"));
