// LLVM's C API exposes no exception-model option. Keep this tiny adapter
// tied to the same LLVM headers as llvm-sys; it changes only this machine,
// so native and WASI codegen can coexist in a process without global flags.
#include "llvm/MC/MCAsmInfo.h"
#include "llvm/Target/TargetMachine.h"

extern "C" void perry_llvm_enable_wasm_eh(void *machine) {
    auto *target = static_cast<llvm::TargetMachine *>(machine);
    target->Options.ExceptionModel = llvm::ExceptionHandling::Wasm;
    const_cast<llvm::MCAsmInfo *>(target->getMCAsmInfo())
        ->setExceptionsType(llvm::ExceptionHandling::Wasm);
}
