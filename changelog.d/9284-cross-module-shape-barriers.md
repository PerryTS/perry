### Fixed

- Cross-module function localization now leaves object-shape barriers in their
  source modules, preventing unrelated safe argument-shape clones in importers
  from falling back to generic dispatch.
