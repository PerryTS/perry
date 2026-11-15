`js_get_string_pointer_unified` (the runtime's shared "value to string pointer"
entry, also behind every UI `Str` argument) decodes by the tag alone. Its raw
arm returned a 4-byte-aligned word with clear top 16 bits above 0x10000 as a
StringHeader address, so an address-shaped Number (a denormal double) was read
as a string header: `Buffer.isEncoding(e)` crashed with SIGSEGV. Such a Number
is now converted to its string form like every other Number, as in Node.
