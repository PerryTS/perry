Fixed stale or mismatched `libperry_runtime` archives passing `perry doctor` and
then failing during native linking with undefined runtime symbols. Runtime
archives now carry a compiler build identity that `perry doctor` and compile
pipelines verify before linking, with actionable rebuild and reinstall guidance.
