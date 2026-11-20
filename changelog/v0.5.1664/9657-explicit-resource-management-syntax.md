### Fixed

- JavaScript `using` and `await using` declarations no longer emit
  `UsingDeclNotEnabled` parse warnings. Perry now enables SWC's
  explicit-resource-management syntax for JavaScript inputs, matching the
  already-enabled TypeScript path and the runtime support for resource
  disposal.
