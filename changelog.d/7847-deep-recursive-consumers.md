**Deeply nested values now raise a catchable error during serialization and
cloning instead of exhausting the native stack.** `JSON.stringify` and
`structuredClone` cap recursive traversal at 1,000 nested containers and remain
usable after the error is caught.
