### Improved

- Reduced native output for large dependency graphs by removing obsolete
  program-wide class metadata augmentation and by keeping imported JSON
  serialized until `JSON.parse` runs at startup. On the 4,743-module OpenCode
  source build this cut the Windows executable from 1,685.4 MiB to 1,143.7 MiB
  and reduced module-codegen time by 12.8%.
- Added opt-in hybrid size optimization with
  `PERRY_LL_PREOPT_OPTNONE_INSTRS`: generated functions above the configured
  instruction cap skip the LLVM middle-end while ordinary siblings in the same
  codegen unit remain eligible for `-Os`.
