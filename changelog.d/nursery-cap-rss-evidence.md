### Changed

- Keep the adaptive copying nursery's steady-state ceiling byte-denominated:
  survivor object-size censuses remain diagnostic after the first copying
  minor, low mortality no longer shrinks the earned scale, and only minors
  that were actually nursery-cap-due may rate influx growth.
