- Fixed the Windows release and npm packages after the LLVM 22 dynamic-link
  change: `LLVM-C.dll` is now shipped beside `perry.exe`, and both packaging
  paths fail closed if the executable's required LLVM runtime is absent. This
  prevents extracted or npm-installed Windows builds from exiting with loader
  error `0xC0000135` on machines without a separate LLVM installation.
