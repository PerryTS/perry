# SQL prefix stress probe (#9935)

This fixture uses the reported `drizzle-orm@0.44.7` query-construction code,
without a database, driver, credentials, or network requests. It checks the
exact SQL text and every parameter for the reported four-predicate SELECT and
a 40-extra-predicate variant that repeatedly grows the chunk/parameter arrays.
It retains previous query results and collects between constructing the head
and appending the tail, and after materializing SQL.

This is an investigation probe. Passing does **not** establish that the rare
Linux production failure is fixed, or exclude a driver/transaction/async path.
Failing gives a smaller boundary to investigate before involving MySQL.

Install and check against the Node version in `.node-version`:

```sh
cd tests/release/packages/drizzle-sql-prefix
npm ci --ignore-scripts
node --expose-gc --experimental-strip-types entry.ts > node-out.txt
diff -u expected.txt node-out.txt
```

Run the usual release fixture using a compiler and static runtime built from
the same source tree:

```sh
PERRY_BIN=/absolute/path/to/perry bash fixture.sh
```

For a standalone stress run, compile once and execute the same binary under
both normal collection and forced moving collection:

```sh
"$PERRY_BIN" compile entry.ts -o out
PERRY_SQL_PREFIX_ITERATIONS=10000 ./out
PERRY_SQL_PREFIX_ITERATIONS=10000 PERRY_GC_DIAG=1 \
  PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 \
  ./out > forced.out 2> forced.log
```

For additional collection windows *inside* Drizzle's loops, use a scheduled
run (loop polls must be present in the compiled binary):

```sh
PERRY_SQL_PREFIX_ITERATIONS=10 PERRY_GC_DIAG=1 \
  PERRY_GC_SCHEDULE_SEED=9935 PERRY_GC_SCHEDULE_RATE=0.05 \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
  ./out > scheduled.out 2> scheduled.log
```

Start with the small scheduled run: removing allocation pacing can produce
thousands of collections in only ten iterations. Scale it separately from the
ordinary 1,000-iteration acceptance run.

Record exit status, the exact build commit/platform, seed, stdout, and stderr.
Use an external timeout for unattended runs. An explicit `gc()` count only
proves that the fixture called `gc`; a moving-GC result also needs runtime
evidence that copying/evacuation actually ran. Inspect `[gc-copy-minor] ran`
records and `[gc-fromspace-protect]` lines containing `retired_set=` before
claiming that coverage.
If a verifier fails before a SQL assertion, preserve that diagnostic separately;
it is not proof that SQL lost its prefix. Issue #9942's hang/leak may be related,
but this fixture does not assume that connection.
