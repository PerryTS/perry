`x.message[k]` and `x.name[k]` read the property of whatever `x.message` /
`x.name` holds. Codegen guesses those two names are strings (Error fields) and
lowers the read through the string index entry; the entry already handed a
non-string heap object to the ordinary `[[Get]]`, but a Proxy (and any other
non-heap, non-string value) read `undefined`. A Solid store's
`store.session.message[sessionID]` therefore lost every message: opencode's
session view stayed empty after a prompt. The entry now takes the ordinary
`[[Get]]` for every value that is not a string.
