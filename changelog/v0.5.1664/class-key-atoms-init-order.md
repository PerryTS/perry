Harden class key lists: `build_longlived_keys_array` now creates each key's
pool atom (same eligibility as the pool) when the list is built, so a class key
list holds the pool's key atoms regardless of module init order. Previously a
class registered before the pool that names one of its keys held a non-atom
key, so the megamorphic slot confirm could not compare against it. Read-site
keys currently get atoms in a startup batch, so this removes an init-order
dependency rather than fixing an observed program-level failure.
