**Full collections now retire shape descriptors that no live object or
restamping cache owns.** A full trace records every shaped receiver, and a
synchronous sweep prunes only records absent from that complete census.
Minor and budgeted cycles remain conservative. Generated-module ids and
shape/transition caches retain exact ownership, while unstable transition
entries validate their target before stamping. In the claude-code census,
uncarried descriptors without an owner fell from 34,501 to zero.
