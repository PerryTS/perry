Move ArrayBuffer backing ownership through worker messages. ArrayBuffers now
hold native bytes in their GC wrapper, and successful transfer passes that
allocation to the receiving heap without either serialized or receiver copies.
Legacy inline and borrowed backing relocates once at transfer time. Validation
and clone failures leave every sender attached, and repeated views share one
received backing. Unread messages, detach, GC, and thread exit release ownership.
