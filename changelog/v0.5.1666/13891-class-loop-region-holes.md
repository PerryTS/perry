A loop over a class instance now runs its loop region's fast copy: 24 -> 12
instructions per iteration for a plain `.js` class, 18 -> 13 for a `.ts` class
with declared or initialized fields. A class family layout may hold holes for
life, so the region's prime now serves it with a presence test on the object
(a re-check never passes a word with the presence bit; only the full entry
guard proves presence). A loop-invariant bound (`n` in `i < n`) is
entry-tested as a Number instead of re-checking every iteration, and a read
the region proves a Number stays a raw double in the fast copy.
The region word now names a key's storage slot, not its position in the key
list: an instance whose keys were stored out of declaration order no longer
has the fast copy store one key into another key's slot.
