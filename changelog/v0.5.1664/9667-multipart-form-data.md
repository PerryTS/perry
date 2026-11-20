### Fixed

- `Request.formData()` and `Response.formData()` now parse
  `multipart/form-data` bodies into ordered text and `File` entries instead of
  treating upload boundaries as URL-encoded keys. Parsed files preserve their
  name, MIME type, size, and binary bytes.
