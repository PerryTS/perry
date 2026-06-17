// Issue #1021 follow-up: RxJS-style barrels re-export classes from
// `src/internal/*`, while consumers import from the package index. The
// cross-module method metadata must keep the defining file's prefix, not
// the barrel's prefix, or the link step references method symbols that no
// module emitted.

import { Observable, Subject } from "./fixtures/issue_1021_rxjs_reexport_methods/index.js";

function read(obs: Observable): string {
  return obs.subscribe();
}

const subject = new Subject("seed");

console.log(read(new Observable("plain")));
console.log(subject.next("value"));
console.log(subject.error("boom"));
console.log(subject.next("after"));
