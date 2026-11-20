Documented why the deferred `this`-patch closure roots in object-literal lowering
need no explicit release: `temp_root` truncation is a stack cut, so the enclosing
scope's single truncate already releases every root pushed after it. A #6972
review finding read the missing per-root release as a leak; it is not, and the
absence of a comment saying so is what made it look like one.
