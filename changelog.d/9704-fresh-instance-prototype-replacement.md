### Fixed

- Fresh instances now observe class prototype methods replaced after class
  registration. The method inliner keeps runtime dispatch for prototype chains
  exposed or mutated anywhere in the module, including helper functions and
  closures. (#9239)
