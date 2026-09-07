(lldb) target create /Users/amlug/projects/perry/codex-json-fastpaths-artifacts/string11-lifetime-worker
Current executable set to '/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/string11-lifetime-worker' (arm64).
(lldb) disassemble -n _RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger
string11-lifetime-worker`perry_runtime::gc::policy::gc_check_trigger:
string11-lifetime-worker[0x1004323c0] <+0>:    stp    x28, x27, [sp, #-0x40]!
string11-lifetime-worker[0x1004323c4] <+4>:    stp    x22, x21, [sp, #0x10]
string11-lifetime-worker[0x1004323c8] <+8>:    stp    x20, x19, [sp, #0x20]
string11-lifetime-worker[0x1004323cc] <+12>:   stp    x29, x30, [sp, #0x30]
string11-lifetime-worker[0x1004323d0] <+16>:   add    x29, sp, #0x30
string11-lifetime-worker[0x1004323d4] <+20>:   sub    sp, sp, #0x900
string11-lifetime-worker[0x1004323d8] <+24>:   ldr    xzr, [sp]
string11-lifetime-worker[0x1004323dc] <+28>:   adrp   x0, 2772
string11-lifetime-worker[0x1004323e0] <+32>:   add    x0, x0, #0x8a8            ; perry_runtime::gc::policy::GC_BUDGETED_STEP_ACTIVE::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004323e4] <+36>:   ldr    x8, [x0]
string11-lifetime-worker[0x1004323e8] <+40>:   blr    x8
string11-lifetime-worker[0x1004323ec] <+44>:   ldrb   w8, [x0]
string11-lifetime-worker[0x1004323f0] <+48>:   tbnz   w8, #0x0, 0x100432594     ; <+468>
string11-lifetime-worker[0x1004323f4] <+52>:   adrp   x0, 2772
string11-lifetime-worker[0x1004323f8] <+56>:   add    x0, x0, #0xa28            ; perry_runtime::gc::policy::GC_FLAGS::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004323fc] <+60>:   ldr    x8, [x0]
string11-lifetime-worker[0x100432400] <+64>:   blr    x8
string11-lifetime-worker[0x100432404] <+68>:   ldrb   w8, [x0]
string11-lifetime-worker[0x100432408] <+72>:   tbnz   w8, #0x1, 0x100432594     ; <+468>
string11-lifetime-worker[0x10043240c] <+76>:   adrp   x0, 2772
string11-lifetime-worker[0x100432410] <+80>:   add    x0, x0, #0x8d8            ; perry_runtime::gc::policy::GC_BUDGETED_CYCLE_ACTIVE::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432414] <+84>:   ldr    x9, [x0]
string11-lifetime-worker[0x100432418] <+88>:   blr    x9
string11-lifetime-worker[0x10043241c] <+92>:   tbz    w8, #0x0, 0x100432428     ; <+104>
string11-lifetime-worker[0x100432420] <+96>:   ldrb   w8, [x0]
string11-lifetime-worker[0x100432424] <+100>:  tbz    w8, #0x0, 0x100432594     ; <+468>
string11-lifetime-worker[0x100432428] <+104>:  adrp   x8, 2987
string11-lifetime-worker[0x10043242c] <+108>:  add    x8, x8, #0x944            ; perry_runtime::gc::policy::GC_UNSAFE_ZONES
string11-lifetime-worker[0x100432430] <+112>:  ldapr  w8, [x8]
string11-lifetime-worker[0x100432434] <+116>:  cmp    w8, #0x0
string11-lifetime-worker[0x100432438] <+120>:  b.gt   0x100432594               ; <+468>
string11-lifetime-worker[0x10043243c] <+124>:  adrp   x8, 2766
string11-lifetime-worker[0x100432440] <+128>:  ldr    w19, [x8, #0xa30]
string11-lifetime-worker[0x100432444] <+132>:  cmp    w19, #0x300
string11-lifetime-worker[0x100432448] <+136>:  b.hs   0x1004324e8               ; <+296>
string11-lifetime-worker[0x10043244c] <+140>:  adrp   x8, 2766
string11-lifetime-worker[0x100432450] <+144>:  ldr    x8, [x8, #0x70]
string11-lifetime-worker[0x100432454] <+148>:  cmn    x8, #0x1
string11-lifetime-worker[0x100432458] <+152>:  b.eq   0x1004324cc               ; <+268>
string11-lifetime-worker[0x10043245c] <+156>:  mrs    x9, TPIDRRO_EL0
string11-lifetime-worker[0x100432460] <+160>:  and    x9, x9, #0xfffffffffffffff8
string11-lifetime-worker[0x100432464] <+164>:  ldr    x8, [x9, x8, lsl #3]
string11-lifetime-worker[0x100432468] <+168>:  cbz    x8, 0x1004324cc           ; <+268>
string11-lifetime-worker[0x10043246c] <+172>:  add    x8, x8, x19, lsl #3
string11-lifetime-worker[0x100432470] <+176>:  ldr    x8, [x8, #0x1e8]
string11-lifetime-worker[0x100432474] <+180>:  cbz    x8, 0x1004324e8           ; <+296>
string11-lifetime-worker[0x100432478] <+184>:  ldr    x8, [x8]
string11-lifetime-worker[0x10043247c] <+188>:  cbz    x8, 0x10043250c           ; <+332>
string11-lifetime-worker[0x100432480] <+192>:  adrp   x0, 2772
string11-lifetime-worker[0x100432484] <+196>:  add    x0, x0, #0x800            ; perry_runtime::gc::policy::GC_DEFERRED_REQUEST::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432488] <+200>:  ldr    x8, [x0]
string11-lifetime-worker[0x10043248c] <+204>:  blr    x8
string11-lifetime-worker[0x100432490] <+208>:  ldrb   w8, [x0]
string11-lifetime-worker[0x100432494] <+212>:  sub    w9, w8, #0x9
string11-lifetime-worker[0x100432498] <+216>:  mov    w10, #0x3                 ; =3 
string11-lifetime-worker[0x10043249c] <+220>:  cmp    w8, #0x8
string11-lifetime-worker[0x1004324a0] <+224>:  csel   w9, w9, w10, hi
string11-lifetime-worker[0x1004324a4] <+228>:  cmp    w9, #0x2
string11-lifetime-worker[0x1004324a8] <+232>:  b.eq   0x1004324c0               ; <+256>
string11-lifetime-worker[0x1004324ac] <+236>:  and    w9, w9, #0xff
string11-lifetime-worker[0x1004324b0] <+240>:  cmp    w9, #0x3
string11-lifetime-worker[0x1004324b4] <+244>:  b.eq   0x1004324c4               ; <+260>
string11-lifetime-worker[0x1004324b8] <+248>:  mov    w8, #0xa                  ; =10 
string11-lifetime-worker[0x1004324bc] <+252>:  b      0x1004324c4               ; <+260>
string11-lifetime-worker[0x1004324c0] <+256>:  mov    w8, #0xb                  ; =11 
string11-lifetime-worker[0x1004324c4] <+260>:  strb   w8, [x0]
string11-lifetime-worker[0x1004324c8] <+264>:  b      0x100432594               ; <+468>
string11-lifetime-worker[0x1004324cc] <+268>:  mov    x20, x0
string11-lifetime-worker[0x1004324d0] <+272>:  bl     0x100adb430               ; perry_runtime::tls_hot::hot_uncached
string11-lifetime-worker[0x1004324d4] <+276>:  mov    x8, x0
string11-lifetime-worker[0x1004324d8] <+280>:  mov    x0, x20
string11-lifetime-worker[0x1004324dc] <+284>:  add    x8, x8, x19, lsl #3
string11-lifetime-worker[0x1004324e0] <+288>:  ldr    x8, [x8, #0x1e8]
string11-lifetime-worker[0x1004324e4] <+292>:  cbnz   x8, 0x100432478           ; <+184>
string11-lifetime-worker[0x1004324e8] <+296>:  adrp   x8, 2652
string11-lifetime-worker[0x1004324ec] <+300>:  add    x8, x8, #0x9c0            ; perry_runtime::gc::roots::GC_ROOT_LOCK_DEPTH
string11-lifetime-worker[0x1004324f0] <+304>:  mov    x19, x0
string11-lifetime-worker[0x1004324f4] <+308>:  mov    x0, x8
string11-lifetime-worker[0x1004324f8] <+312>:  bl     0x100ad889c               ; <perry_runtime::tls_hot::HotKey<perry_runtime::module_require::path_registry::PathModuleRegistry>>::get_slow
string11-lifetime-worker[0x1004324fc] <+316>:  mov    x8, x0
string11-lifetime-worker[0x100432500] <+320>:  mov    x0, x19
string11-lifetime-worker[0x100432504] <+324>:  ldr    x8, [x8]
string11-lifetime-worker[0x100432508] <+328>:  cbnz   x8, 0x100432480           ; <+192>
string11-lifetime-worker[0x10043250c] <+332>:  ldrb   w8, [x0]
string11-lifetime-worker[0x100432510] <+336>:  tbnz   w8, #0x0, 0x10043254c     ; <+396>
string11-lifetime-worker[0x100432514] <+340>:  mov    x20, x0
string11-lifetime-worker[0x100432518] <+344>:  bl     0x100432c04               ; perry_runtime::gc::policy::gc_budgeted_due_trigger
string11-lifetime-worker[0x10043251c] <+348>:  mov    x8, x0
string11-lifetime-worker[0x100432520] <+352>:  mov    x0, x20
string11-lifetime-worker[0x100432524] <+356>:  tst    w8, #0xff
string11-lifetime-worker[0x100432528] <+360>:  b.ne   0x10043254c               ; <+396>
string11-lifetime-worker[0x10043252c] <+364>:  adrp   x0, 2772
string11-lifetime-worker[0x100432530] <+368>:  add    x0, x0, #0x908            ; perry_runtime::gc::policy::GC_OLD_RECLAIM_IN_PROGRESS::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432534] <+372>:  ldr    x8, [x0]
string11-lifetime-worker[0x100432538] <+376>:  blr    x8
string11-lifetime-worker[0x10043253c] <+380>:  mov    x19, x0
string11-lifetime-worker[0x100432540] <+384>:  mov    x0, x20
string11-lifetime-worker[0x100432544] <+388>:  ldrb   w8, [x19]
string11-lifetime-worker[0x100432548] <+392>:  tbz    w8, #0x0, 0x1004326a4     ; <+740>
string11-lifetime-worker[0x10043254c] <+396>:  ldrb   w8, [x0]
string11-lifetime-worker[0x100432550] <+400>:  tbz    w8, #0x0, 0x1004325ac     ; <+492>
string11-lifetime-worker[0x100432554] <+404>:  ldrb   w8, [x0]
string11-lifetime-worker[0x100432558] <+408>:  tbnz   w8, #0x0, 0x10043256c     ; <+428>
string11-lifetime-worker[0x10043255c] <+412>:  bl     0x100432c04               ; perry_runtime::gc::policy::gc_budgeted_due_trigger
string11-lifetime-worker[0x100432560] <+416>:  mov    w8, #0xff                 ; =255 
string11-lifetime-worker[0x100432564] <+420>:  bics   wzr, w8, w0
string11-lifetime-worker[0x100432568] <+424>:  b.eq   0x100432594               ; <+468>
string11-lifetime-worker[0x10043256c] <+428>:  add    x0, sp, #0x8
string11-lifetime-worker[0x100432570] <+432>:  bl     0x1002517f8               ; <perry_runtime::gc::telemetry::GcDebtSnapshot>::current
string11-lifetime-worker[0x100432574] <+436>:  ldp    x8, x9, [sp, #0x8]
string11-lifetime-worker[0x100432578] <+440>:  lsr    x8, x8, #5
string11-lifetime-worker[0x10043257c] <+444>:  add    x8, x8, #0x100
string11-lifetime-worker[0x100432580] <+448>:  adds   x8, x8, x9
string11-lifetime-worker[0x100432584] <+452>:  csinv  x1, x8, xzr, lo
string11-lifetime-worker[0x100432588] <+456>:  add    x0, sp, #0x8
string11-lifetime-worker[0x10043258c] <+460>:  mov    w2, #0x1                  ; =1 
string11-lifetime-worker[0x100432590] <+464>:  bl     0x1004359ec               ; perry_runtime::gc::policy::gc_budgeted_step_work_units_inner_with_progress
string11-lifetime-worker[0x100432594] <+468>:  add    sp, sp, #0x900
string11-lifetime-worker[0x100432598] <+472>:  ldp    x29, x30, [sp, #0x30]
string11-lifetime-worker[0x10043259c] <+476>:  ldp    x20, x19, [sp, #0x20]
string11-lifetime-worker[0x1004325a0] <+480>:  ldp    x22, x21, [sp, #0x10]
string11-lifetime-worker[0x1004325a4] <+484>:  ldp    x28, x27, [sp], #0x40
string11-lifetime-worker[0x1004325a8] <+488>:  ret    
string11-lifetime-worker[0x1004325ac] <+492>:  adrp   x8, 2766
string11-lifetime-worker[0x1004325b0] <+496>:  add    x8, x8, #0xcf8            ; _MergedGlobals + 1104
string11-lifetime-worker[0x1004325b4] <+500>:  ldapr  x8, [x8]
string11-lifetime-worker[0x1004325b8] <+504>:  cbnz   x8, 0x100432908           ; <+1352>
string11-lifetime-worker[0x1004325bc] <+508>:  adrp   x8, 2766
string11-lifetime-worker[0x1004325c0] <+512>:  ldrb   w8, [x8, #0xd00]
string11-lifetime-worker[0x1004325c4] <+516>:  tbnz   w8, #0x0, 0x1004325f8     ; <+568>
string11-lifetime-worker[0x1004325c8] <+520>:  adrp   x8, 2766
string11-lifetime-worker[0x1004325cc] <+524>:  add    x8, x8, #0xeb8            ; _MergedGlobals + 1552
string11-lifetime-worker[0x1004325d0] <+528>:  ldapr  x8, [x8]
string11-lifetime-worker[0x1004325d4] <+532>:  cbnz   x8, 0x100432924           ; <+1380>
string11-lifetime-worker[0x1004325d8] <+536>:  adrp   x8, 2766
string11-lifetime-worker[0x1004325dc] <+540>:  ldrb   w8, [x8, #0xec0]
string11-lifetime-worker[0x1004325e0] <+544>:  cbnz   w8, 0x1004325f8           ; <+568>
string11-lifetime-worker[0x1004325e4] <+548>:  mov    x19, x0
string11-lifetime-worker[0x1004325e8] <+552>:  bl     0x10041abc4               ; perry_runtime::gc::roots::registered_root_scanners_block_budgeted_gc
string11-lifetime-worker[0x1004325ec] <+556>:  mov    x8, x0
string11-lifetime-worker[0x1004325f0] <+560>:  mov    x0, x19
string11-lifetime-worker[0x1004325f4] <+564>:  tbz    w8, #0x0, 0x100432554     ; <+404>
string11-lifetime-worker[0x1004325f8] <+568>:  mov    x19, x0
string11-lifetime-worker[0x1004325fc] <+572>:  bl     0x100432c04               ; perry_runtime::gc::policy::gc_budgeted_due_trigger
string11-lifetime-worker[0x100432600] <+576>:  and    w8, w0, #0xff
string11-lifetime-worker[0x100432604] <+580>:  cmp    w8, #0x2
string11-lifetime-worker[0x100432608] <+584>:  b.gt   0x100432690               ; <+720>
string11-lifetime-worker[0x10043260c] <+588>:  sub    w8, w8, #0x1
string11-lifetime-worker[0x100432610] <+592>:  cmp    w8, #0x2
string11-lifetime-worker[0x100432614] <+596>:  mov    x0, x19
string11-lifetime-worker[0x100432618] <+600>:  b.hs   0x100432554               ; <+404>
string11-lifetime-worker[0x10043261c] <+604>:  bl     0x10040d3e4               ; perry_runtime::gc::json_defer::should_defer
string11-lifetime-worker[0x100432620] <+608>:  tbz    w0, #0x0, 0x100432754     ; <+916>
string11-lifetime-worker[0x100432624] <+612>:  adrp   x0, 2772
string11-lifetime-worker[0x100432628] <+616>:  add    x0, x0, #0x830            ; perry_runtime::gc::policy::GC_SAFEPOINT_PENDING::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x10043262c] <+620>:  ldr    x8, [x0]
string11-lifetime-worker[0x100432630] <+624>:  blr    x8
string11-lifetime-worker[0x100432634] <+628>:  ldrb   w9, [x0]
string11-lifetime-worker[0x100432638] <+632>:  tbnz   w9, #0x0, 0x100432594     ; <+468>
string11-lifetime-worker[0x10043263c] <+636>:  mov    x8, x0
string11-lifetime-worker[0x100432640] <+640>:  adrp   x0, 2772
string11-lifetime-worker[0x100432644] <+644>:  add    x0, x0, #0x950            ; perry_runtime::gc::policy::GC_SAFEPOINT_DEFER_ARENA_BASE::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432648] <+648>:  ldr    x9, [x0]
string11-lifetime-worker[0x10043264c] <+652>:  blr    x9
string11-lifetime-worker[0x100432650] <+656>:  mov    x9, x0
string11-lifetime-worker[0x100432654] <+660>:  adrp   x0, 2772
string11-lifetime-worker[0x100432658] <+664>:  add    x0, x0, #0xde8            ; perry_runtime::arena::block::ARENA_TOTAL_BYTES::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x10043265c] <+668>:  ldr    x10, [x0]
string11-lifetime-worker[0x100432660] <+672>:  blr    x10
string11-lifetime-worker[0x100432664] <+676>:  ldr    x10, [x0]
string11-lifetime-worker[0x100432668] <+680>:  str    x10, [x9]
string11-lifetime-worker[0x10043266c] <+684>:  mov    w9, #0x1                  ; =1 
string11-lifetime-worker[0x100432670] <+688>:  strb   w9, [x8]
string11-lifetime-worker[0x100432674] <+692>:  adrp   x8, 2964
string11-lifetime-worker[0x100432678] <+696>:  add    x8, x8, #0x890            ; _MergedGlobals.1886 + 952
string11-lifetime-worker[0x10043267c] <+700>:  ldadd  x9, x8, [x8]
string11-lifetime-worker[0x100432680] <+704>:  adrp   x8, 2766
string11-lifetime-worker[0x100432684] <+708>:  add    x8, x8, #0x68             ; PERRY_GC_POLL_ARMED
string11-lifetime-worker[0x100432688] <+712>:  ldadd  w9, w8, [x8]
string11-lifetime-worker[0x10043268c] <+716>:  b      0x100432594               ; <+468>
string11-lifetime-worker[0x100432690] <+720>:  cmp    w8, #0x3
string11-lifetime-worker[0x100432694] <+724>:  mov    x0, x19
string11-lifetime-worker[0x100432698] <+728>:  b.ne   0x100432554               ; <+404>
string11-lifetime-worker[0x10043269c] <+732>:  mov    w22, #0x1                 ; =1 
string11-lifetime-worker[0x1004326a0] <+736>:  b      0x100432758               ; <+920>
string11-lifetime-worker[0x1004326a4] <+740>:  mov    w8, #0x1                  ; =1 
string11-lifetime-worker[0x1004326a8] <+744>:  strb   w8, [x19]
string11-lifetime-worker[0x1004326ac] <+748>:  adrp   x0, 2772
string11-lifetime-worker[0x1004326b0] <+752>:  add    x0, x0, #0x890            ; perry_runtime::gc::policy::GC_OLD_RECLAIM_PENDING::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004326b4] <+756>:  ldr    x8, [x0]
string11-lifetime-worker[0x1004326b8] <+760>:  blr    x8
string11-lifetime-worker[0x1004326bc] <+764>:  strb   wzr, [x0]
string11-lifetime-worker[0x1004326c0] <+768>:  mov    w0, #0x0                  ; =0 
string11-lifetime-worker[0x1004326c4] <+772>:  bl     0x1002684a4               ; <perry_runtime::gc::roots::scan_mode::ManualGcScanGuard>::force_full_scan
string11-lifetime-worker[0x1004326c8] <+776>:  mov    x20, x0
string11-lifetime-worker[0x1004326cc] <+780>:  adrp   x8, 2766
string11-lifetime-worker[0x1004326d0] <+784>:  add    x8, x8, #0xe78            ; _MergedGlobals + 1488
string11-lifetime-worker[0x1004326d4] <+788>:  ldapr  x8, [x8]
string11-lifetime-worker[0x1004326d8] <+792>:  cbnz   x8, 0x1004329c8           ; <+1544>
string11-lifetime-worker[0x1004326dc] <+796>:  adrp   x8, 2766
string11-lifetime-worker[0x1004326e0] <+800>:  ldrb   w9, [x8, #0xe80]
string11-lifetime-worker[0x1004326e4] <+804>:  mov    w8, #0x2                  ; =2 
string11-lifetime-worker[0x1004326e8] <+808>:  cbz    w9, 0x1004329dc           ; <+1564>
string11-lifetime-worker[0x1004326ec] <+812>:  adrp   x0, 2772
string11-lifetime-worker[0x1004326f0] <+816>:  add    x0, x0, #0x7a0            ; perry_runtime::gc::policy::GC_STEP_BYTES::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004326f4] <+820>:  ldr    x9, [x0]
string11-lifetime-worker[0x1004326f8] <+824>:  blr    x9
string11-lifetime-worker[0x1004326fc] <+828>:  ldr    x9, [x0]
string11-lifetime-worker[0x100432700] <+832>:  adrp   x0, 2772
string11-lifetime-worker[0x100432704] <+836>:  add    x0, x0, #0x848            ; perry_runtime::gc::policy::GC_NEXT_TRIGGER_BYTES::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432708] <+840>:  ldr    x10, [x0]
string11-lifetime-worker[0x10043270c] <+844>:  blr    x10
string11-lifetime-worker[0x100432710] <+848>:  ldr    x10, [x0]
string11-lifetime-worker[0x100432714] <+852>:  adrp   x0, 2772
string11-lifetime-worker[0x100432718] <+856>:  add    x0, x0, #0x818            ; perry_runtime::gc::policy::GC_MALLOC_COUNT_STEP::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x10043271c] <+860>:  ldr    x11, [x0]
string11-lifetime-worker[0x100432720] <+864>:  blr    x11
string11-lifetime-worker[0x100432724] <+868>:  ldr    x11, [x0]
string11-lifetime-worker[0x100432728] <+872>:  adrp   x0, 2772
string11-lifetime-worker[0x10043272c] <+876>:  add    x0, x0, #0x878            ; perry_runtime::gc::policy::GC_NEXT_MALLOC_TRIGGER::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432730] <+880>:  ldr    x12, [x0]
string11-lifetime-worker[0x100432734] <+884>:  blr    x12
string11-lifetime-worker[0x100432738] <+888>:  ldr    x12, [x0]
string11-lifetime-worker[0x10043273c] <+892>:  adrp   x0, 2772
string11-lifetime-worker[0x100432740] <+896>:  add    x0, x0, #0x7e8            ; perry_runtime::gc::policy::GC_TRIGGER_BUMPED::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432744] <+900>:  ldr    x13, [x0]
string11-lifetime-worker[0x100432748] <+904>:  blr    x13
string11-lifetime-worker[0x10043274c] <+908>:  ldrb   w13, [x0]
string11-lifetime-worker[0x100432750] <+912>:  b      0x1004329e0               ; <+1568>
string11-lifetime-worker[0x100432754] <+916>:  mov    w22, #0x0                 ; =0 
string11-lifetime-worker[0x100432758] <+920>:  adrp   x8, 2766
string11-lifetime-worker[0x10043275c] <+924>:  add    x8, x8, #0xeb8            ; _MergedGlobals + 1552
string11-lifetime-worker[0x100432760] <+928>:  ldapr  x8, [x8]
string11-lifetime-worker[0x100432764] <+932>:  cbnz   x8, 0x100432940           ; <+1408>
string11-lifetime-worker[0x100432768] <+936>:  adrp   x8, 2766
string11-lifetime-worker[0x10043276c] <+940>:  ldrb   w8, [x8, #0xec0]
string11-lifetime-worker[0x100432770] <+944>:  tbz    w8, #0x0, 0x100432818     ; <+1112>
string11-lifetime-worker[0x100432774] <+948>:  adrp   x0, 2772
string11-lifetime-worker[0x100432778] <+952>:  add    x0, x0, #0xde8            ; perry_runtime::arena::block::ARENA_TOTAL_BYTES::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x10043277c] <+956>:  ldr    x8, [x0]
string11-lifetime-worker[0x100432780] <+960>:  blr    x8
string11-lifetime-worker[0x100432784] <+964>:  ldr    x20, [x0]
string11-lifetime-worker[0x100432788] <+968>:  adrp   x0, 2772
string11-lifetime-worker[0x10043278c] <+972>:  add    x0, x0, #0x830            ; perry_runtime::gc::policy::GC_SAFEPOINT_PENDING::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432790] <+976>:  ldr    x8, [x0]
string11-lifetime-worker[0x100432794] <+980>:  blr    x8
string11-lifetime-worker[0x100432798] <+984>:  mov    x19, x0
string11-lifetime-worker[0x10043279c] <+988>:  ldrb   w8, [x0]
string11-lifetime-worker[0x1004327a0] <+992>:  tbz    w8, #0x0, 0x1004328b8     ; <+1272>
string11-lifetime-worker[0x1004327a4] <+996>:  adrp   x0, 2772
string11-lifetime-worker[0x1004327a8] <+1000>: add    x0, x0, #0x950            ; perry_runtime::gc::policy::GC_SAFEPOINT_DEFER_ARENA_BASE::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004327ac] <+1004>: ldr    x8, [x0]
string11-lifetime-worker[0x1004327b0] <+1008>: blr    x8
string11-lifetime-worker[0x1004327b4] <+1012>: ldr    x21, [x0]
string11-lifetime-worker[0x1004327b8] <+1016>: adrp   x8, 2766
string11-lifetime-worker[0x1004327bc] <+1020>: add    x8, x8, #0xd88            ; _MergedGlobals + 1248
string11-lifetime-worker[0x1004327c0] <+1024>: ldapr  x8, [x8]
string11-lifetime-worker[0x1004327c4] <+1028>: cbnz   x8, 0x100432a28           ; <+1640>
string11-lifetime-worker[0x1004327c8] <+1032>: adrp   x8, 2766
string11-lifetime-worker[0x1004327cc] <+1036>: ldr    x8, [x8, #0xd90]
string11-lifetime-worker[0x1004327d0] <+1040>: adds   x8, x21, x8
string11-lifetime-worker[0x1004327d4] <+1044>: csinv  x8, x8, xzr, lo
string11-lifetime-worker[0x1004327d8] <+1048>: cmp    x20, x8
string11-lifetime-worker[0x1004327dc] <+1052>: b.lo   0x100432594               ; <+468>
string11-lifetime-worker[0x1004327e0] <+1056>: ldrb   w8, [x19]
string11-lifetime-worker[0x1004327e4] <+1060>: cbz    w8, 0x100432818           ; <+1112>
string11-lifetime-worker[0x1004327e8] <+1064>: strb   wzr, [x19]
string11-lifetime-worker[0x1004327ec] <+1068>: adrp   x8, 2766
string11-lifetime-worker[0x1004327f0] <+1072>: ldr    w9, [x8, #0x68]
string11-lifetime-worker[0x1004327f4] <+1076>: adrp   x8, 2766
string11-lifetime-worker[0x1004327f8] <+1080>: add    x8, x8, #0x68             ; PERRY_GC_POLL_ARMED
string11-lifetime-worker[0x1004327fc] <+1084>: cbz    w9, 0x100432818           ; <+1112>
string11-lifetime-worker[0x100432800] <+1088>: sub    w10, w9, #0x1
string11-lifetime-worker[0x100432804] <+1092>: mov    x11, x9
string11-lifetime-worker[0x100432808] <+1096>: cas    w11, w10, [x8]
string11-lifetime-worker[0x10043280c] <+1100>: cmp    w11, w9
string11-lifetime-worker[0x100432810] <+1104>: mov    x9, x11
string11-lifetime-worker[0x100432814] <+1108>: b.ne   0x1004327fc               ; <+1084>
string11-lifetime-worker[0x100432818] <+1112>: bl     0x1004deebc               ; perry_runtime::arena::walk::arena_in_use_bytes
string11-lifetime-worker[0x10043281c] <+1116>: mov    x20, x0
string11-lifetime-worker[0x100432820] <+1120>: bl     0x100460310               ; perry_runtime::gc::telemetry::malloc_object_count
string11-lifetime-worker[0x100432824] <+1124>: mov    x21, x0
string11-lifetime-worker[0x100432828] <+1128>: mov    w0, #0x1                  ; =1 
string11-lifetime-worker[0x10043282c] <+1132>: bl     0x1002684a4               ; <perry_runtime::gc::roots::scan_mode::ManualGcScanGuard>::force_full_scan
string11-lifetime-worker[0x100432830] <+1136>: mov    x19, x0
string11-lifetime-worker[0x100432834] <+1140>: adrp   x8, 2766
string11-lifetime-worker[0x100432838] <+1144>: add    x8, x8, #0xe78            ; _MergedGlobals + 1488
string11-lifetime-worker[0x10043283c] <+1148>: ldapr  x8, [x8]
string11-lifetime-worker[0x100432840] <+1152>: cbnz   x8, 0x100432954           ; <+1428>
string11-lifetime-worker[0x100432844] <+1156>: adrp   x8, 2766
string11-lifetime-worker[0x100432848] <+1160>: ldrb   w8, [x8, #0xe80]
string11-lifetime-worker[0x10043284c] <+1164>: cbz    w8, 0x100432964           ; <+1444>
string11-lifetime-worker[0x100432850] <+1168>: adrp   x0, 2772
string11-lifetime-worker[0x100432854] <+1172>: add    x0, x0, #0x7a0            ; perry_runtime::gc::policy::GC_STEP_BYTES::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432858] <+1176>: ldr    x8, [x0]
string11-lifetime-worker[0x10043285c] <+1180>: blr    x8
string11-lifetime-worker[0x100432860] <+1184>: ldr    x8, [x0]
string11-lifetime-worker[0x100432864] <+1188>: adrp   x0, 2772
string11-lifetime-worker[0x100432868] <+1192>: add    x0, x0, #0x848            ; perry_runtime::gc::policy::GC_NEXT_TRIGGER_BYTES::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x10043286c] <+1196>: ldr    x9, [x0]
string11-lifetime-worker[0x100432870] <+1200>: blr    x9
string11-lifetime-worker[0x100432874] <+1204>: ldr    x9, [x0]
string11-lifetime-worker[0x100432878] <+1208>: adrp   x0, 2772
string11-lifetime-worker[0x10043287c] <+1212>: add    x0, x0, #0x818            ; perry_runtime::gc::policy::GC_MALLOC_COUNT_STEP::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432880] <+1216>: ldr    x10, [x0]
string11-lifetime-worker[0x100432884] <+1220>: blr    x10
string11-lifetime-worker[0x100432888] <+1224>: ldr    x10, [x0]
string11-lifetime-worker[0x10043288c] <+1228>: adrp   x0, 2772
string11-lifetime-worker[0x100432890] <+1232>: add    x0, x0, #0x878            ; perry_runtime::gc::policy::GC_NEXT_MALLOC_TRIGGER::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432894] <+1236>: ldr    x11, [x0]
string11-lifetime-worker[0x100432898] <+1240>: blr    x11
string11-lifetime-worker[0x10043289c] <+1244>: ldr    x11, [x0]
string11-lifetime-worker[0x1004328a0] <+1248>: adrp   x0, 2772
string11-lifetime-worker[0x1004328a4] <+1252>: add    x0, x0, #0x7e8            ; perry_runtime::gc::policy::GC_TRIGGER_BUMPED::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004328a8] <+1256>: ldr    x12, [x0]
string11-lifetime-worker[0x1004328ac] <+1260>: blr    x12
string11-lifetime-worker[0x1004328b0] <+1264>: ldrb   w12, [x0]
string11-lifetime-worker[0x1004328b4] <+1268>: b      0x100432968               ; <+1448>
string11-lifetime-worker[0x1004328b8] <+1272>: adrp   x8, 2766
string11-lifetime-worker[0x1004328bc] <+1276>: add    x8, x8, #0xd88            ; _MergedGlobals + 1248
string11-lifetime-worker[0x1004328c0] <+1280>: ldapr  x8, [x8]
string11-lifetime-worker[0x1004328c4] <+1284>: cbnz   x8, 0x100432a48           ; <+1672>
string11-lifetime-worker[0x1004328c8] <+1288>: adrp   x0, 2772
string11-lifetime-worker[0x1004328cc] <+1292>: add    x0, x0, #0x950            ; perry_runtime::gc::policy::GC_SAFEPOINT_DEFER_ARENA_BASE::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004328d0] <+1296>: ldr    x8, [x0]
string11-lifetime-worker[0x1004328d4] <+1300>: blr    x8
string11-lifetime-worker[0x1004328d8] <+1304>: str    x20, [x0]
string11-lifetime-worker[0x1004328dc] <+1308>: ldrb   w8, [x19]
string11-lifetime-worker[0x1004328e0] <+1312>: tbnz   w8, #0x0, 0x100432594     ; <+468>
string11-lifetime-worker[0x1004328e4] <+1316>: mov    w8, #0x1                  ; =1 
string11-lifetime-worker[0x1004328e8] <+1320>: strb   w8, [x19]
string11-lifetime-worker[0x1004328ec] <+1324>: adrp   x9, 2964
string11-lifetime-worker[0x1004328f0] <+1328>: add    x9, x9, #0x890            ; _MergedGlobals.1886 + 952
string11-lifetime-worker[0x1004328f4] <+1332>: ldadd  x8, x9, [x9]
string11-lifetime-worker[0x1004328f8] <+1336>: adrp   x9, 2766
string11-lifetime-worker[0x1004328fc] <+1340>: add    x9, x9, #0x68             ; PERRY_GC_POLL_ARMED
string11-lifetime-worker[0x100432900] <+1344>: ldadd  w8, w8, [x9]
string11-lifetime-worker[0x100432904] <+1348>: b      0x100432594               ; <+468>
string11-lifetime-worker[0x100432908] <+1352>: mov    x19, x0
string11-lifetime-worker[0x10043290c] <+1356>: bl     0x100ab7144               ; <std::sync::once_lock::OnceLock<bool>>::initialize::<<std::sync::once_lock::OnceLock<bool>>::get_or_init<perry_runtime::gc::gc_scavenge_enabled::{closure#0}>::{closure#0}, !>
string11-lifetime-worker[0x100432910] <+1360>: mov    x0, x19
string11-lifetime-worker[0x100432914] <+1364>: adrp   x8, 2766
string11-lifetime-worker[0x100432918] <+1368>: ldrb   w8, [x8, #0xd00]
string11-lifetime-worker[0x10043291c] <+1372>: tbz    w8, #0x0, 0x1004325c8     ; <+520>
string11-lifetime-worker[0x100432920] <+1376>: b      0x1004325f8               ; <+568>
string11-lifetime-worker[0x100432924] <+1380>: mov    x19, x0
string11-lifetime-worker[0x100432928] <+1384>: bl     0x100ab7534               ; <std::sync::once_lock::OnceLock<bool>>::initialize::<<std::sync::once_lock::OnceLock<bool>>::get_or_init<perry_runtime::gc::policy::gc_moving_loop_polls_enabled::{closure#0}>::{closure#0}, !>
string11-lifetime-worker[0x10043292c] <+1388>: mov    x0, x19
string11-lifetime-worker[0x100432930] <+1392>: adrp   x8, 2766
string11-lifetime-worker[0x100432934] <+1396>: ldrb   w8, [x8, #0xec0]
string11-lifetime-worker[0x100432938] <+1400>: cbz    w8, 0x1004325e4           ; <+548>
string11-lifetime-worker[0x10043293c] <+1404>: b      0x1004325f8               ; <+568>
string11-lifetime-worker[0x100432940] <+1408>: bl     0x100ab7534               ; <std::sync::once_lock::OnceLock<bool>>::initialize::<<std::sync::once_lock::OnceLock<bool>>::get_or_init<perry_runtime::gc::policy::gc_moving_loop_polls_enabled::{closure#0}>::{closure#0}, !>
string11-lifetime-worker[0x100432944] <+1412>: adrp   x8, 2766
string11-lifetime-worker[0x100432948] <+1416>: ldrb   w8, [x8, #0xec0]
string11-lifetime-worker[0x10043294c] <+1420>: tbnz   w8, #0x0, 0x100432774     ; <+948>
string11-lifetime-worker[0x100432950] <+1424>: b      0x100432818               ; <+1112>
string11-lifetime-worker[0x100432954] <+1428>: bl     0x100ab748c               ; <std::sync::once_lock::OnceLock<bool>>::initialize::<<std::sync::once_lock::OnceLock<bool>>::get_or_init<perry_runtime::gc::policy::gc_trace_enabled::{closure#0}>::{closure#0}, !>
string11-lifetime-worker[0x100432958] <+1432>: adrp   x8, 2766
string11-lifetime-worker[0x10043295c] <+1436>: ldrb   w8, [x8, #0xe80]
string11-lifetime-worker[0x100432960] <+1440>: cbnz   w8, 0x100432850           ; <+1168>
string11-lifetime-worker[0x100432964] <+1444>: mov    w12, #0x2                 ; =2 
string11-lifetime-worker[0x100432968] <+1448>: sturb  w22, [x29, #-0x38]
string11-lifetime-worker[0x10043296c] <+1452>: stp    x8, x9, [x29, #-0x60]
string11-lifetime-worker[0x100432970] <+1456>: stp    x10, x11, [x29, #-0x50]
string11-lifetime-worker[0x100432974] <+1460>: sturb  w12, [x29, #-0x40]
string11-lifetime-worker[0x100432978] <+1464>: bl     0x10066dcf0               ; perry_runtime::gc::roots::stack_maps::ensure_built
string11-lifetime-worker[0x10043297c] <+1468>: add    x0, sp, #0x8
string11-lifetime-worker[0x100432980] <+1472>: sub    x1, x29, #0x60
string11-lifetime-worker[0x100432984] <+1476>: mov    w2, #0x0                  ; =0 
string11-lifetime-worker[0x100432988] <+1480>: mov    w3, #0x0                  ; =0 
string11-lifetime-worker[0x10043298c] <+1484>: bl     0x1002d5b40               ; perry_runtime::gc::gc_collect_minor_with_trigger_inner
string11-lifetime-worker[0x100432990] <+1488>: add    x1, sp, #0x8
string11-lifetime-worker[0x100432994] <+1492>: tbz    w22, #0x0, 0x1004329a4    ; <+1508>
string11-lifetime-worker[0x100432998] <+1496>: mov    x0, x21
string11-lifetime-worker[0x10043299c] <+1500>: bl     0x100435014               ; perry_runtime::gc::policy::gc_finish_malloc_trigger_collection
string11-lifetime-worker[0x1004329a0] <+1504>: b      0x1004329ac               ; <+1516>
string11-lifetime-worker[0x1004329a4] <+1508>: mov    x0, x20
string11-lifetime-worker[0x1004329a8] <+1512>: bl     0x100434d00               ; perry_runtime::gc::policy::gc_finish_arena_trigger_collection
string11-lifetime-worker[0x1004329ac] <+1516>: cbz    w19, 0x100432594          ; <+468>
string11-lifetime-worker[0x1004329b0] <+1520>: adrp   x0, 2773
string11-lifetime-worker[0x1004329b4] <+1524>: add    x0, x0, #0x2e0            ; perry_runtime::gc::roots::scan_mode::CONSERVATIVE_STACK_SCAN_OVERRIDE::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x1004329b8] <+1528>: ldr    x8, [x0]
string11-lifetime-worker[0x1004329bc] <+1532>: blr    x8
string11-lifetime-worker[0x1004329c0] <+1536>: mov    w8, #0xff                 ; =255 
string11-lifetime-worker[0x1004329c4] <+1540>: b      0x1004324c4               ; <+260>
string11-lifetime-worker[0x1004329c8] <+1544>: bl     0x100ab748c               ; <std::sync::once_lock::OnceLock<bool>>::initialize::<<std::sync::once_lock::OnceLock<bool>>::get_or_init<perry_runtime::gc::policy::gc_trace_enabled::{closure#0}>::{closure#0}, !>
string11-lifetime-worker[0x1004329cc] <+1548>: adrp   x8, 2766
string11-lifetime-worker[0x1004329d0] <+1552>: ldrb   w9, [x8, #0xe80]
string11-lifetime-worker[0x1004329d4] <+1556>: mov    w8, #0x2                  ; =2 
string11-lifetime-worker[0x1004329d8] <+1560>: cbnz   w9, 0x1004326ec           ; <+812>
string11-lifetime-worker[0x1004329dc] <+1564>: mov    w13, #0x2                 ; =2 
string11-lifetime-worker[0x1004329e0] <+1568>: sturb  w8, [x29, #-0x38]
string11-lifetime-worker[0x1004329e4] <+1572>: stp    x9, x10, [x29, #-0x60]
string11-lifetime-worker[0x1004329e8] <+1576>: stp    x11, x12, [x29, #-0x50]
string11-lifetime-worker[0x1004329ec] <+1580>: sturb  w13, [x29, #-0x40]
string11-lifetime-worker[0x1004329f0] <+1584>: add    x0, sp, #0x8
string11-lifetime-worker[0x1004329f4] <+1588>: sub    x1, x29, #0x60
string11-lifetime-worker[0x1004329f8] <+1592>: bl     0x1002d87f8               ; perry_runtime::gc::gc_collect_full_mark_sweep_with_trigger
string11-lifetime-worker[0x1004329fc] <+1596>: add    x0, sp, #0x8
string11-lifetime-worker[0x100432a00] <+1600>: bl     0x100272a4c               ; <perry_runtime::gc::telemetry::GcCollectOutcome>::emit_after_current
string11-lifetime-worker[0x100432a04] <+1604>: cbz    w20, 0x100432a20          ; <+1632>
string11-lifetime-worker[0x100432a08] <+1608>: adrp   x0, 2773
string11-lifetime-worker[0x100432a0c] <+1612>: add    x0, x0, #0x2e0            ; perry_runtime::gc::roots::scan_mode::CONSERVATIVE_STACK_SCAN_OVERRIDE::{K#0}::{closure#1}::__RUST_STD_INTERNAL_VAL
string11-lifetime-worker[0x100432a10] <+1616>: ldr    x8, [x0]
string11-lifetime-worker[0x100432a14] <+1620>: blr    x8
string11-lifetime-worker[0x100432a18] <+1624>: mov    w8, #0xff                 ; =255 
string11-lifetime-worker[0x100432a1c] <+1628>: strb   w8, [x0]
string11-lifetime-worker[0x100432a20] <+1632>: strb   wzr, [x19]
string11-lifetime-worker[0x100432a24] <+1636>: b      0x100432594               ; <+468>
string11-lifetime-worker[0x100432a28] <+1640>: bl     0x100ab7aac               ; <std::sync::once_lock::OnceLock<usize>>::initialize::<<std::sync::once_lock::OnceLock<usize>>::get_or_init<perry_runtime::gc::heap_budget::gc_moving_defer_slack_dyn_bytes::{closure#0}>::{closure#0}, !>
string11-lifetime-worker[0x100432a2c] <+1644>: adrp   x8, 2766
string11-lifetime-worker[0x100432a30] <+1648>: ldr    x8, [x8, #0xd90]
string11-lifetime-worker[0x100432a34] <+1652>: adds   x8, x21, x8
string11-lifetime-worker[0x100432a38] <+1656>: csinv  x8, x8, xzr, lo
string11-lifetime-worker[0x100432a3c] <+1660>: cmp    x20, x8
string11-lifetime-worker[0x100432a40] <+1664>: b.lo   0x100432594               ; <+468>
string11-lifetime-worker[0x100432a44] <+1668>: b      0x1004327e0               ; <+1056>
string11-lifetime-worker[0x100432a48] <+1672>: bl     0x100ab7aac               ; <std::sync::once_lock::OnceLock<usize>>::initialize::<<std::sync::once_lock::OnceLock<usize>>::get_or_init<perry_runtime::gc::heap_budget::gc_moving_defer_slack_dyn_bytes::{closure#0}>::{closure#0}, !>
string11-lifetime-worker[0x100432a4c] <+1676>: b      0x1004328c8               ; <+1288>
(lldb) quit
