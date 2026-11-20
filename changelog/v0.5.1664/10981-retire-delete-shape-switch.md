Removed the `PERRY_DELETE_SHAPE_TRANSITION` switch. Ordinary property deletes
now always change the receiver's shape, so cached property reads cannot match
a deleted slot when the switch is set to `0`.
