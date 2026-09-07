### Fixed

- Preserve property creation order when enumerating ordinary properties on
  buffers, data views, and buffer-backed typed arrays. Updating a property now
  keeps its position, while deleting and recreating it appends the key.
