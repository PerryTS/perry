### Fixed

- Preserve `Headers` instances passed through the shorthand `fetch(url, {
  headers })` option so their entries are sent like the explicit
  `headers: headers` form.
