Make dense `Array.splice()` and `Array.unshift()` update GC element metadata
from only the inserted slots instead of rebuilding it from every live element.
Pointer-free and all-pointer layouts remain exact, mixed layouts fall back to
conservative scanning, and old-array dirty-page coverage follows moved
survivors. Repeated middle splice and front insertion no longer time out at
100,000 operations.
