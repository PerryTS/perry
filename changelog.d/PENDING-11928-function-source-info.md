Retained `Function.prototype.toString` text now occupies one cold read-only
source blob per module instead of ordinary literal pages. Ordinary compiled
functions carry relocation-free `(relative offset, length)` fields in their
existing immutable function-info record, removing their eager source-registry
calls and hash-map inserts at startup; unloadable images, runtime-generated
functions, and raw class method/accessor bodies retain the owning or borrowed
compatibility registration paths required by their lifetimes and ABIs.
