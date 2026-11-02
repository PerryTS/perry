Dynamic closure calls that pad missing arguments with 
( and the exact arms of ) now fill
one fixed array once and read constant indices, instead of a bounds-checked
branch per argument in every arm. Behavior is unchanged; the two functions are
much smaller in every linked binary.
