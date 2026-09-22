### Fixed

- Preserve exceptions thrown by custom iterators during array push spread when using debug or test runtime archives, so JavaScript `catch` handlers receive them instead of the process aborting (#11010).
