// Read-only gate before the irreversible wrapper publication. A publish exit
// status is insufficient: npm can hold a successful upload in scanning limbo.
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";

export function platformPackages(manifest) {
  const packages = manifest?.packages;
  if (!Array.isArray(packages) || packages.length < 2 ||
      packages.filter(p => p.name === "@perryts/perry").length !== 1 ||
      new Set(packages.map(p => p.name)).size !== packages.length ||
      packages.some(p => typeof p.name !== "string" ||
        typeof p.version !== "string" || !p.version ||
        !/^[0-9a-f]{40}$/.test(p.sha1))) {
    throw new Error("Invalid npm publication manifest");
  }
  return packages.filter(p => p.name !== "@perryts/perry");
}

export async function waitForPlatforms(packages, {
  budgetMs = 45 * 60_000,
  requestMs = 15_000,
  intervalMs = 20_000,
  fetchImpl = fetch,
  now = Date.now,
  pause = sleep,
  log = console.log,
} = {}) {
  if (!packages.length || !Number.isFinite(budgetMs) || budgetMs <= 0 ||
      !Number.isFinite(requestMs) || requestMs <= 0 ||
      !Number.isFinite(intervalMs) || intervalMs <= 0) {
    throw new Error("Invalid platform visibility budget or package set");
  }
  const deadline = now() + budgetMs;
  let pending = [...packages];
  while (pending.length && now() < deadline) {
    const unseen = [];
    for (const pkg of pending) {
      const remaining = deadline - now();
      if (remaining <= 0) {
        unseen.push(pkg);
        continue;
      }
      let visible = false;
      try {
        // Abort bounds the entire fetch AND body read, including a connected
        // server that never finishes its response, not just socket inactivity.
        const response = await fetchImpl(
          "https://registry.npmjs.org/" + encodeURIComponent(pkg.name),
          { signal: AbortSignal.timeout(Math.max(1, Math.floor(Math.min(requestMs, remaining)))) },
        );
        if (response.ok) {
          const body = await response.json();
          visible = Boolean(body.time?.[pkg.version]) &&
            body.versions?.[pkg.version]?.dist?.shasum === pkg.sha1;
        } else {
          await response.body?.cancel();
        }
      } catch {
        // A failed/aborted request is unseen, never permission to publish.
      }
      if (visible && now() <= deadline) log(`  visible: ${pkg.name}@${pkg.version}`);
      else unseen.push(pkg);
    }
    pending = unseen;
    if (pending.length && now() < deadline) {
      await pause(Math.min(intervalMs, deadline - now()));
    }
  }
  if (pending.length) {
    throw new Error("Platform visibility budget expired: " +
      pending.map(p => `${p.name}@${p.version}`).join(", "));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const manifest = JSON.parse(await readFile(process.argv[2], "utf8"));
    await waitForPlatforms(platformPackages(manifest), {
      budgetMs: Number(process.env.VISIBILITY_WAIT_MIN ?? "45") * 60_000,
    });
  } catch (error) {
    console.error(`::error::${error.message}`);
    process.exitCode = 1;
  }
}
