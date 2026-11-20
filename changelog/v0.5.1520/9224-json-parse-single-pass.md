### Performance

- `JSON.parse` now validates and constructs values in one strict parser pass,
  eliminating the preliminary full-document validation scan.
