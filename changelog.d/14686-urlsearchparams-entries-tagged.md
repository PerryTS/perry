A `URLSearchParams` keeps its entries array, and each `[name, value]` pair in it, as
tagged pointer values. Both were stored as bare heap addresses in value slots (a field
of the params object, the elements of the entries array), which the collector and
`JSON.stringify` had to recognise by guessing that an address-shaped word is a pointer.
No behaviour change.
