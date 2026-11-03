Fixed re-adding a property after deleting a configurable, non-writable data
property. The delete now clears the old property attributes, so a later
assignment creates a writable, enumerable property in the expected key order.
