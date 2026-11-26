- **GC checks at microtask drain ends.** The "is a full collection owed?" and
  trigger-ladder questions were asked at the end of every microtask drain, with
  the same answer each time. The poll now reports a clean "nothing due", and
  only then does the drain boundary skip the repeats. `due_arm` reads the
  mature-heap size only after a debt term crosses its band. The order of the
  checks is unchanged. Timer-heavy and I/O-heavy programs run fewer
  instructions (a timer loop -43%, an I/O loop -14%).
