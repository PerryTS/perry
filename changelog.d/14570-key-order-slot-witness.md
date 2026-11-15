Check mode (`PERRY_VERIFY_FACTS=1`) no longer reports a false proof for a
class whose constructor stores a key in a branch, loop or `try` (#14485's
first-store layout). The field-slot witness now states the layout's claim: a
key's slot is its own storage word, named by the object's key list or, once
the runtime orders the object by its overflow table, by the key's overflow
entry; a key that may be absent can also find the hole there, which the read
sends to its generic path. Default builds are unchanged.
