**iOS Foundation Models:** restore `perry-ui-ios` cross-compilation by removing
an orphaned call to the old manual Promise-pinning API. Foundation Models now
relies on `js_promise_new_cross_thread`, which owns the non-moving allocation
and pin/unpin lifecycle through settlement.
