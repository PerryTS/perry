### Fixed

- Route global `console.dirxml` through the existing log output path and handle
  `profile`, `profileEnd`, and `timeStamp` as output-suppressing no-ops that
  still evaluate arguments in order and return `undefined`. This is a partial
  console parity slice related to #9202; the umbrella audit remains open.
