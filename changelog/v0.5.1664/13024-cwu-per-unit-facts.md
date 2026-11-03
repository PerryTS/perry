### Changed

- Whole-program facts (`perry-wpfacts`) are now solved per unit: a closed-world solver composes function summaries over static-call SCCs with object tokens per birth site, and each unit (a module, or an SCC of the module-scope reference graph) publishes only what other units can name, as contracts. Units on the same dependency level are solved in parallel. A fact that depends on an unknown caller or value is only taken behind a contract the census checks.
