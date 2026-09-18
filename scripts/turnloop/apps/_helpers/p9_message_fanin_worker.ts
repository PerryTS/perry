// The Worker half of `p9_worker_message_fanin.ts`: post once, then keep the
// worker alive in one of three ways. The three cases are the experiment.
//
//   P9_FANIN_LINGER unset/0 : return immediately.
//   P9_FANIN_LINGER=1       : a `setInterval`, an explicit keep-alive handle.
//   P9_FANIN_LINGER=2       : nothing but `parentPort.on("message")`, which in
//                             Node keeps a Worker alive on its own.
//
// The third is what a worker-pool shape actually writes, and what the RSS probe
// originally used to hold N loops open while the parent measured.
import { parentPort } from "node:worker_threads";

const linger = process.env.P9_FANIN_LINGER ?? "0";

if (linger === "2") {
  // Registered BEFORE the post, so "the listener was not installed yet" cannot
  // be the explanation for anything observed.
  parentPort?.on("message", () => {});
}

parentPort?.postMessage("ready");

if (linger === "1") {
  setInterval(() => {}, 1000);
}
