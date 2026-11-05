Modules whose names sanitize to the same symbol prefix
(`providers/google-vertex-chat.ts` and `providers/google-vertex/chat.ts`)
now get distinct prefixes: after collection each module of a colliding group
has its name tagged with a hash of the name, and the driver's path-derived
prefixes follow. Their symbols used to merge, which failed OpenCode v2.1.2's
link and broke small programs at run time ("value is not a function").
