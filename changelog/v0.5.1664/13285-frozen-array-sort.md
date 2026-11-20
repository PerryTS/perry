Fix default array sorting to honor frozen indexed properties instead of mutating dense frozen arrays. Preserve successful empty, singleton, and all-hole sorts, and nonmutating `toSorted()` behavior.
