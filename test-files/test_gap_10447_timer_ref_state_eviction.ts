// #10447: a SCHEDULED timer's ref state and handle kind must survive any
// number of later timers. The id -> ref-state registry is bounded (#6084), but
// it evicted the oldest 65,536+ ids whether or not they were still scheduled.
// After that many later timers a live `unref()`'d timer read as ref'd again —
// the process stayed alive until it fired and ran the callback the program had
// detached — and `.hasRef()` / `.ref()` / `.unref()` / `.constructor` / `+t`
// stopped resolving on the handle at all.
//
// Every "BUG" callback below belongs to an unref'd timer: once the last ref'd
// timer has fired nothing keeps the loop alive, so none of them may run (and
// the process exits promptly instead of waiting 1-1.5 s for them).

const CHURN = 70000; // > 65,536

function churn(n: number): void {
  for (let i = 0; i < n; i++) clearTimeout(setTimeout(() => {}, 1000));
}

function show(label: string, t: any): void {
  const hasRef = typeof t.hasRef === "function" ? t.hasRef() : "missing";
  const ctor = t.constructor ? t.constructor.name : "missing";
  let line = `${label}: hasRef=${hasRef} ctor=${ctor}`;
  // A Timeout coerces to its numeric id (an Immediate does not).
  if (ctor !== "Immediate") line += ` primitive=${!Number.isNaN(+t)}`;
  console.log(line);
}

process.on("exit", () => console.log("exit"));

// --- below-cap control: unaffected before and after the fix ---
const control = setTimeout(() => console.log("BUG: control unref'd timeout fired"), 1500);
control.unref();
churn(1000);
show("control after 1000 timers", control);

// --- subjects scheduled BEFORE more than 65,536 later timers ---
const unrefTimeout = setTimeout(() => console.log("BUG: unref'd timeout fired"), 1500);
unrefTimeout.unref();

const unrefInterval = setInterval(() => {
  console.log("BUG: unref'd interval fired");
  clearInterval(unrefInterval);
}, 1000);
unrefInterval.unref();

const unrefLater = setTimeout(() => console.log("BUG: timeout unref'd after churn fired"), 1500);

const reRef = setTimeout(() => console.log("re-ref'd timeout fired"), 1);
reRef.unref();

const immediate = setImmediate(() => {});

const refd = setTimeout(() => console.log("last ref'd timeout fired"), 20);

churn(CHURN);
console.log(`churned ${CHURN} timers`);

show("unref'd timeout", unrefTimeout);
show("unref'd interval", unrefInterval);
show("immediate", immediate);
show("ref'd timeout", refd);

// unref() / ref() on a live handle that has 70k later timers behind it.
show("timeout before unref()", unrefLater);
unrefLater.unref();
show("timeout after unref()", unrefLater);
show("timeout before ref()", reRef);
reRef.ref();
show("timeout after ref()", reRef);

// post-clear state on a recent handle is kept (the bounded part of #6084)
const recent = setTimeout(() => console.log("BUG: cleared timeout fired"), 1);
recent.unref();
clearTimeout(recent);
show("recent cleared timeout", recent);

console.log("main done");
