Read-site holder entries, class entries and method-site entries no longer
register their sites or root their holders. A site memo may store an object's
address only when the object is stable: old and never-move. Prototypes are
marked never-move when they become prototypes, a copying or fallback minor
promotes them at their first survival, and old-page defragmentation and idle
compaction never move them. Declared class prototypes, function default
prototypes and builtin prototypes (`%Object.prototype%` included) are born old.
A receiver ShapeId pins every object the entry names, so the entry needs no
root; a class relink pins a displaced prototype that a memo named, and
replacing any accessor function word moves the holder's ShapeId. The
`HOLDER_SITES` and `METHOD_SITES` registries and their minor-GC scanners are
deleted; keyed read sites keep rooting only their keys until keys are
immortal atoms. Tests show each premise fails when sabotaged.
