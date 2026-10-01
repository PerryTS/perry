- runtime: the object store fast paths (`put_value` set/define/prime,
  `field_set_by_name` fast paths, the delete path's stable-tombstone admission)
  no longer list the typed-layout-intact bit among their blocking flags: nothing
  sets it on an object any more. The array side (`header_gc_slots`) keeps it
  (charter step 5, P4 flip).
