import { saveCache } from "@actions/cache";
import { getState, warning } from "@actions/core";
import { CacheState } from "./action";
import { getCacheKey } from "./utils";
(async () => {
  const state: CacheState = JSON.parse(getState("cache"));
  if (state.cacheEnabled && !state.cacheHit) {
    const cacheKey = getCacheKey(state.url);

    try {
      await saveCache([state.bunPath], cacheKey);
      process.exit(0);
    } catch (error) {
      warning("Failed to save Bun to cache.");
    }
  }
})();
