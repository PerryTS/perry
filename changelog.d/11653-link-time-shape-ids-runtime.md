The shape table can now adopt a ShapeId the compiler assigned. A reserved band
of ids is never drawn from the counter; the ordinary mint takes a requested id
from that band on a first mint of those facts in an agent, marks the record as
held by generated code so it is never pruned, and returns the existing id on
every later mint of the same facts. Class and plain-literal seed entry points
use it, and a test shows a worker resolves a replayed object to the static id.
