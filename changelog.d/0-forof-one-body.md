for-of over an array lowers its body once (#13645). The counted iterator
record of #13655 split into an array arm and a protocol arm, and each arm
carried its own copy of the user body (Supabase code +5.5 MB, +2.65%, at the
same function count). The loop now has one body and two heads: the entry
verdict is tested once, the indexed head steps like `i < a.length; a[i]` with
the source's declared element type, the protocol head runs the record's Step,
and both enter the same body with the step value bound; the record's update
and completion state are shared. A binding that is the step value's only
reader takes the element without a string alias demote on the indexed head.
