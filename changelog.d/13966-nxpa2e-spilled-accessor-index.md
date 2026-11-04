The key index names a spilled accessor pair as it names an inline one. derive_word
now gives every public accessor of an indexed list an ACCESSOR_ENTRY whose index
word is the pair's storage slot, with PAIR_SPILLED when the slot lies past the
record's inline slots. The probe (first bucket and continued) and the slow walk
(own_entry -> accessor_in) read the pair with one load, inline or from the spill
(index_pair_bits), instead of re-reading the key's attributes and storage slot on
each read. The word is a fact of the immutable key list, its attributes and the
inline slot count; no cache. An empty spill slot declines exactly as before.
A runtime test checks that the index names every spilled pair and that both
readers return the attribute walk's word.
