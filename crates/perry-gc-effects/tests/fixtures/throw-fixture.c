/* Machine-code fixtures for native-origin throws. The demangled leaves are
   the public throw API, so symbol matching is exercised in every format. */
volatile int throw_fixture_sink;
#define LEAF(c, symbol) void c(int v) __asm__(symbol); void c(int v) { throw_fixture_sink = v; }
LEAF(core_leaf, "_ZN13perry_runtime9exception10throw_leaf10throw_core17h0000000000000000E")
LEAF(caught_leaf, "_ZN13perry_runtime9exception10throw_leaf14rethrow_caught17h0000000000000000E")
LEAF(program_leaf, "_ZN13perry_runtime9exception10throw_leaf19throw_program_value17h0000000000000000E")
LEAF(open_leaf, "_ZN13perry_runtime9exception10throw_leaf10throw_open17h0000000000000000E")
typedef void (*program_body)(int);
void fixture_js(program_body body, int v) { body(v); }
void fixture_js_validate(program_body body, int v) { if (v < 0) core_leaf(v); body(v); }
void fixture_none(program_body body, int v) { caught_leaf(v); fixture_js(body, v); }
void fixture_program(int v) { core_leaf(v); open_leaf(v); program_leaf(v); }
void fixture_core(int v) { core_leaf(v); }
void fixture_passes(int v) { program_leaf(v); }
void fixture_open(int v) { open_leaf(v); }
extern void js_throw(int v);
void fixture_raw(int v) { PLANT throw_fixture_sink = v; }
