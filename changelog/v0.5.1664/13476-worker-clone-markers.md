### Fixed

- Share private worker clone/transfer markers with global `structuredClone`, ignore primitive marks and preserve builtin clone exemptions.
- Store marker identity in one GC-rooted weak table so live marks survive moving collections without retaining dead targets or copying a marker set for every message.
