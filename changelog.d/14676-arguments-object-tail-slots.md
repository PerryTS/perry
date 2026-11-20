An arguments object given more own keys than its inline slots hold (`arguments["q" + i] = i` twelve times, or a
delete and re-add) keeps working: index reads, `delete arguments["0"]`, `Object.keys` and the parameter aliases answer
as node does. Its private birth word and alias state were a tail at the end of its cell, which the inline-slot capacity
counted as slots, so a store overwrote it and the next index access crashed. Both now live in the object's meta
record (`ObjectMeta::arguments_birth`, `ObjectMeta::arguments`); the cell holds property slots only.
