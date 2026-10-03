A compiled class-method site whose method name has its prototype guard set (a
prototype member of that name was assigned, deleted or redefined anywhere) no
longer tries to learn a receiver word on every call: no learned word can be
consulted behind a set guard, so the miss edge passes no site and dispatches
directly. Part of the #10507 `decimal_class` regression (1,911 -> 1,478
instructions per operation together with the own-key scan fix).
