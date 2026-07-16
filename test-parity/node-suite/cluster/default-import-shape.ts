// #3687: node:cluster default-import EventEmitter + primary setup state.
//
// Deterministic primary-process parity (no real worker forking): the default
// import (an EventEmitter) and the `import * as` namespace diverge for the
// EventEmitter method surface, callable arity matches Node, setupPrimary()
// updates cluster.settings, and default-import listener bookkeeping
// (on/emit/eventNames/listenerCount/removeListener) round-trips.
import clusterDefault from "node:cluster";
import * as clusterNamespace from "node:cluster";

const def = clusterDefault as Record<string, any>;
const ns = clusterNamespace as Record<string, any>;

console.log("typeof default:", typeof clusterDefault);
console.log(
  "isPrimary:",
  def.isPrimary,
  "isMaster:",
  def.isMaster,
  "isWorker:",
  def.isWorker,
);
console.log(
  "schedulingPolicy:",
  def.schedulingPolicy,
  "SCHED_RR:",
  def.SCHED_RR,
  "SCHED_NONE:",
  def.SCHED_NONE,
);

// Default import exposes the EventEmitter surface; the namespace import does not.
for (
  const name of [
    "fork",
    "disconnect",
    "setupPrimary",
    "setupMaster",
    "Worker",
    "on",
    "addListener",
    "once",
    "emit",
    "eventNames",
    "listenerCount",
  ]
) {
  console.log(
    `default ${name}: ${typeof def[name]}/${
      def[name]?.length
    } | namespace ${name}: ${typeof ns[name]}`,
  );
}

// setupPrimary() updates cluster.settings with Node-compatible keys.
def.setupPrimary({ execArgv: ["--probe-flag"] });
console.log(
  "settings.execArgv isArray:",
  Array.isArray(def.settings?.execArgv),
);
console.log("settings.execArgv:", JSON.stringify(def.settings?.execArgv));

// Default-import EventEmitter registration / bookkeeping.
let fired = 0;
const listener = (n: number) => {
  fired += n;
};
const onResult = def.on("custom", listener);
console.log("on returns self:", onResult === clusterDefault);
console.log("eventNames after on:", def.eventNames().map(String).join(","));
console.log("listenerCount(custom):", def.listenerCount("custom"));
console.log("emit returns:", def.emit("custom", 41));
console.log("fired:", fired);
def.removeListener("custom", listener);
console.log("listenerCount after remove:", def.listenerCount("custom"));
def.once("once-evt", () => {});
console.log(
  "eventNames before removeAll:",
  def.eventNames().map(String).sort().join(","),
);
def.removeAllListeners();
console.log("eventNames after removeAll:", def.eventNames().join(","));
