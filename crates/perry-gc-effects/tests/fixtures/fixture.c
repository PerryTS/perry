
typedef int (*fp)(int);
volatile int fixture_sink;
int js_proxy_get(int x) { fixture_sink = x; return x ^ 0x5a; }
void fixture_collect(void) { __asm__ volatile("" ::: "memory"); }
void js_throw(void) { js_proxy_get(1); }
extern int unknown_external(int);
extern void _Unwind_RaiseException(void *);
void fixture_poll(void) { fixture_collect(); }
void fixture_collect_js(void) { js_proxy_get(0); }
extern void *memcpy(void *, const void *, unsigned long);

int leaf_helper(int x) { PLANT return x * 3 + 1; }
int js_back_hot_text(int x) { PLANT return x; }
int alloc_helper(int x) { fixture_collect(); return x + 2; }
int throw_helper(int x) { if (x > 100) js_throw(); return x; }
int reenter_helper(int x) { return js_proxy_get(x) + 3; }
int poll_helper(int x) { fixture_poll(); return x + 4; }
int gc_reaches_js_helper(int x) { fixture_collect_js(); return x + 6; }
int unwind_helper(int x) { _Unwind_RaiseException(0); return x + 8; }
int chain_helper(int x) { return reenter_helper(x) * 5; }
int indirect_helper(fp f, int x) { return f(x) + 7; }
int extern_helper(int x) { return unknown_external(x) + 11; }
int libc_helper(char *d, const char *s, unsigned long n) { memcpy(d, s, n); return 0; }
int switch_helper(int k) {
    switch (k) {
    case 0: return leaf_helper(k);
    case 1: fixture_sink = 48; return 18;
    case 2: fixture_sink = 85; return 31;
    case 3: fixture_sink = 122; return 44;
    case 4: fixture_sink = 159; return 57;
    case 5: fixture_sink = 196; return 70;
    case 6: fixture_sink = 233; return 83;
    case 7: fixture_sink = 270; return 96;
    case 8: fixture_sink = 307; return 109;
    case 9: fixture_sink = 344; return 122;
    case 10: fixture_sink = 381; return 135;
    case 11: fixture_sink = 418; return 148;
    case 12: fixture_sink = 455; return 161;
    case 13: fixture_sink = 492; return 174;
    case 14: fixture_sink = 529; return 187;
    case 15: fixture_sink = 566; return 200;
    case 16: fixture_sink = 603; return 213;
    case 17: fixture_sink = 640; return 226;
    case 18: fixture_sink = 677; return 239;
    case 19: fixture_sink = 714; return 252;
    default: return 0;
    }
}
