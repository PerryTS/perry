wpfacts no longer reads a sync generator's public next()/return()/throw() as
returning the step closure's raw value: the result is an iterator result
{ value, done } whose value joins every yielded value, so a receiver that a
generator yields alongside another shape is no longer proven as the first
shape alone. Output was already correct (the runtime is guarded); only the
fact was wrong.
