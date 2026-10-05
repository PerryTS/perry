import { tmpdir } from "node:os";
import { getInput, setOutput, setFailed, getBooleanInput } from "@actions/core";
import runAction from "./action.js";
import { readVersionFromFile } from "./utils.js";
import { parseRegistries } from "./registry.js";

if (!process.env.RUNNER_TEMP) {
  process.env.RUNNER_TEMP = tmpdir();
}

const registries = parseRegistries(getInput("registries"));

// Backwards compatibility for the `registry-url` and `scope` inputs
const registryUrl = getInput("registry-url");
const scope = getInput("scope");

if (registryUrl) {
  registries.push({
    url: registryUrl,
    scope: scope,
    token: "$BUN_AUTH_TOKEN",
  });
}

runAction({
  version:
    getInput("bun-version") ||
    readVersionFromFile(getInput("bun-version-file")) ||
    readVersionFromFile("package.json", true) ||
    undefined,
  customUrl: getInput("bun-download-url") || undefined,
  registries: registries,
  noCache: getBooleanInput("no-cache") || false,
  token: getInput("token"),
})
  .then(({ version, revision, bunPath, url, cacheHit }) => {
    setOutput("bun-version", version);
    setOutput("bun-revision", revision);
    setOutput("bun-path", bunPath);
    setOutput("bun-download-url", url);
    setOutput("cache-hit", cacheHit);
    process.exit(0);
  })
  .catch((error) => {
    setFailed(error);
    process.exit(1);
  });
