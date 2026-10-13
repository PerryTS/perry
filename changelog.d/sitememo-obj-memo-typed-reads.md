### Changed

- Read sites answer inherited and absent keys through a typed `Obj(S, memo)`
  body (decision 129): once the receiver's ShapeId matches the entry, a deep
  entry's intermediate hops are proven by the one shared proof epoch
  (`PERRY_PROTO_VALIDITY`) recorded at publication instead of a ShapeId
  compare per hop, and a class entry no longer re-reads the receiver's class
  id. Publication marks every hop a prototype (so its structural changes
  advance the epoch); a class relink advances it too. A stale entry declines
  to the one generic entry, whose shape re-proof records the current epoch.
- With `PERRY_CHECK_CONTRACTS=1` while compiling, each typed read body re-runs
  the full shape proof and aborts on a wrong proof; with
  `PERRY_FORCE_GENERIC=1`, every typed read body declines, for the
  differential gate. Builds without them carry no test on any read.
