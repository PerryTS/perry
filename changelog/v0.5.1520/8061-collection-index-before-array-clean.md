### Fixed

- Map and Set indexed reads through array-like runtime fallbacks no longer
  return `NaN`: tag-selected, registry-confirmed collections are routed before
  strict Array pointer validation (#8060).
