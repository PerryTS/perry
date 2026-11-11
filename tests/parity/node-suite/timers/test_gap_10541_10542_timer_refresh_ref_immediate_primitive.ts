// #10541 / #10542: `Timeout.refresh()` ref-state semantics, and
// `Immediate` vs `Timeout` numeric conversion.
//
// #10541: `refresh()` reschedules a timer using its original delay, but per
// Node does NOT touch ref/unref state. Perry's `js_timer_refresh` force-set
// the id ref'd (`set_timer_ref_state(id, true)`), so refreshing an unref'd
// timer re-ref'd it: `hasRef()` flipped to `true` and the (BUG-labelled)
// callback that had been deliberately detached from the event loop ran,
// keeping the process alive until it fired.
//
// #10542: Node's `Timeout` (setTimeout/setInterval) has a numeric
// conversion (`+t` is its internal id) but `Immediate` (setImmediate) does
// not (`+im` is `NaN`) -- Perry gave both handles the Timeout conversion.

process.on("exit", () => console.log("exit"));

function show(label: string, t: any): void {
  const hasRef = typeof t.hasRef === "function" ? t.hasRef() : "missing";
  const ctor = t.constructor ? t.constructor.name : "missing";
  const primitive = +t;
  console.log(
    `${label}: hasRef=${hasRef} ctor=${ctor} typeof(+t)=${typeof primitive} isNaN(+t)=${Number.isNaN(
      primitive,
    )}`,
  );
}

// --- #10541: refresh() must preserve ref state ------------------------------

// An unref'd timeout, refreshed: must stay unref'd. If the bug is present
// this callback runs (BUG) and keeps the process alive for ~1.2s.
const unrefTimeout = setTimeout(
  () => console.log("BUG: unref'd refresh()'d timeout fired"),
  1200,
);
unrefTimeout.unref();
show("unref'd timeout before refresh", unrefTimeout);
unrefTimeout.refresh();
show("unref'd timeout after refresh", unrefTimeout);

// An unref'd interval, refreshed: must stay unref'd. Cleared immediately
// (synchronously, before the event loop ever runs) so it never fires either
// way -- this only exercises the post-refresh() hasRef() state.
const unrefInterval = setInterval(
  () => console.log("BUG: unref'd refresh()'d interval fired"),
  1200,
);
unrefInterval.unref();
show("unref'd interval before refresh", unrefInterval);
unrefInterval.refresh();
show("unref'd interval after refresh", unrefInterval);
clearInterval(unrefInterval);

// A ref'd timeout, refreshed: must stay ref'd, and must still fire (proves
// refresh() itself -- the reschedule -- keeps working).
const refdTimeout = setTimeout(
  () => console.log("ref'd refresh()'d timeout fired"),
  50,
);
show("ref'd timeout before refresh", refdTimeout);
refdTimeout.refresh();
show("ref'd timeout after refresh", refdTimeout);

// unref() then explicit ref() then refresh(): refresh() must not perturb an
// explicit re-ref either (guards a naive fix that always forces ref state
// to false instead of leaving it alone).
const reRefTimeout = setTimeout(
  () => console.log("BUG: reRefTimeout should have been cleared"),
  5000,
);
reRefTimeout.unref();
reRefTimeout.ref();
show("reRef timeout after ref()", reRefTimeout);
reRefTimeout.refresh();
show("reRef timeout after refresh", reRefTimeout);
clearTimeout(reRefTimeout);

// --- #10542: Immediate has no numeric conversion; Timeout/Interval do -------

const immediate = setImmediate(() =>
  console.log("BUG: immediate should have been cleared"),
);
show("immediate", immediate);
console.log(
  "Object.prototype.toString.call(immediate)",
  Object.prototype.toString.call(immediate),
);

const plainTimeout = setTimeout(() => {}, 5000);
show("plain timeout (unfired)", plainTimeout);

// clearTimeout/clearImmediate must still work on the handles above.
clearTimeout(plainTimeout);
clearImmediate(immediate);
console.log("cleared plainTimeout and immediate");

console.log("main done");
