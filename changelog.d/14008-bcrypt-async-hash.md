`await bcrypt.hash(...)` (and `argon2.hash`) no longer hangs in a program
whose only async work is the ext binding's pool job. A pool submission now
installs the turnloop hooks the event pump parks on, so the completion is
collected; before, such a program took the legacy condvar park and waited
forever unless a timer or an I/O module happened to install them.
`bcrypt.hashSync`, `bcrypt.compareSync`, `bcrypt.genSalt` and
`bcrypt.genSaltSync` are now implemented instead of compiling to
"unimplemented API" errors. `hashSync` takes a salt string or a cost, and
salt rounds follow node: a missing or zero count is 10 and any other is
clamped to 4..31.
The generated API reference and native-result inventory include these APIs;
bcrypt uses the same native providers in ext and bundled builds, including
boolean async comparison results.
