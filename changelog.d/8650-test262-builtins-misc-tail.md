Closed the remaining test262 built-ins "misc tail" semantics gaps across the
class-registry construct path, descriptors, prototype chain and proxy
put-value.

Also lowers the raw-handle ratchet 925 -> 923: the converted sites use the
#7341 handle shapes rather than re-baselining, so the debt figure drops with
the change that pays it down.
