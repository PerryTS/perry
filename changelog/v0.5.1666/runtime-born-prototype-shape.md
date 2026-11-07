Startup no longer mints a fresh shape for each runtime prototype object when the
generator and iterator prototypes are sealed: they are born on a reserved shape
lineage, so marking them as prototypes keeps their shape. Locked-prototype
semantics are unchanged; this removes the startup instruction cost added by
locking the generator and iterator prototypes.
