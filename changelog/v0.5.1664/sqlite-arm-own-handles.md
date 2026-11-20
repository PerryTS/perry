Fixed `.get()`, `.all()`, `.run()`, `.raw()`, `.prepare()`, `.exec()` and
`.close()` on handle-backed objects that are not SQLite statements or
databases, such as a fetch Response's `headers`. The SQLite arm of the dynamic
handle dispatch claimed every handle for those method names; since #11919 its
entry points throw for a handle they do not own, so `response.headers.get(...)`
threw "The database connection is not open". The arm now claims only handles
the SQLite registry owns.
