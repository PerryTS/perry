- **fix(gc-root-dominance): `--verify-symbols` read zero `js_*` symbols on every Linux host (#11496).** The parser matched only Mach-O's `_js_` spelling, and ELF symbols carry no leading underscore. It also shelled out to `nm -gj`, whose output does not separate definitions from `U` references.
  - It now parses POSIX `nm -g -P` output, which GNU nm, llvm-nm and Apple nm all emit. That output's type column drops references, and names are accepted with or without the Mach-O prefix.
  - Readers are tried in order, and the first that sees any `js_*` symbol wins:
    1. `PERRY_NM` pins a reader outright.
    2. rustup's `llvm-tools` `llvm-nm`, whose bitcode reader matches rustc's LLVM.
    3. `$LLVM_SYS_221_PREFIX/bin/llvm-nm`.
    4. `llvm-nm-22`.
    5. Homebrew's `llvm-nm`.
    6. `llvm-nm` on `PATH`.
    7. Plain `nm`.
  - A reader that runs but sees nothing, such as GNU/Apple nm on thin-LTO bitcode, falls through to the next. When every reader comes back empty, the error lists what each one reported.
  - `--self-test` now plants an ELF-and-Mach-O fixture with `U` references. Sabotage-checked: the old `_js_`-only parse fails it.
  - Validated on Linux against freshly built `libperry_{runtime,stdlib}.a`. The old script read 0 symbols; the new one reads 3811, and the scanner covers all of them, with both `llvm-nm` 22 and GNU nm.
