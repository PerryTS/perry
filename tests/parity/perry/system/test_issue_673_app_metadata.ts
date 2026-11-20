// Issue #673: app metadata introspection APIs from perry/system.
// The repository-level parity sweep has no perry.toml, so this file guards
// the fallback values while compile.rs unit tests cover perry.toml parsing.

import { getAppBuildNumber, getAppVersion, getBundleId } from "perry/system";

console.log("version", getAppVersion());
console.log("build", getAppBuildNumber());
console.log("bundle", getBundleId());
