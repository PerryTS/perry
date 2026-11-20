// LLVM's C API exposes no function-sections, data-sections or emulated-TLS
// option. Keep this tiny adapter tied to the same LLVM headers as llvm-sys;
// it changes only this machine (fields of its options), so no global flag is
// set.
//
// One input section per function and per global is what lets the link drop
// each one nothing references (DCE layer 0): with the defaults, a module's
// functions share `.text` and its constants share `.data.rel.ro`, so one live
// symbol keeps every function the rest of its section names.
#include "llvm/Target/TargetMachine.h"

extern "C" void perry_llvm_unique_sections(void *machine) {
    auto *target = static_cast<llvm::TargetMachine *>(machine);
    target->Options.FunctionSections = true;
    target->Options.DataSections = true;
}

// Lower every thread-local through `__emutls_get_address` and the
// `__emutls_v.<name>` control variables, the model `clang` and `llc` pick for
// the same triples (`Triple::hasDefaultEmulatedTLS`). The C API leaves it off.
extern "C" void perry_llvm_emulated_tls(void *machine) {
    auto *target = static_cast<llvm::TargetMachine *>(machine);
    target->Options.EmulatedTLS = true;
}
