A property read whose key the probe proves absent from the receiver's own
key list now finishes in one frameless runtime leaf: the record the probe
already holds goes straight into the prototype-chain descriptor lookup
instead of a second dispatch through the inherited-read entry. A hit points
into the immutable descriptor instead of copying the entry. Answers are
unchanged. Absent and inherited reads drop 11–14% of their instructions.
