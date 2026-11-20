// The PID suffix keeps the Function source runtime-derived in native builds.
const cases: [string, string, string][] = [
  [
    "binding_free_loops",
    "var sum=0; for(var i=0;i<8;i++){if(i&1){sum+=i;}} return \"\"+sum;",
    "16"
  ],
  [
    "binding_free_parent_lexical",
    "let n=3; { { n+=2; } } return \"\"+n;",
    "5"
  ],
  [
    "var_hoists_through_block",
    "var total=0; { total+=typeof later===\"undefined\"?1:100; var later=4; } return \"\"+(total+later);",
    "5"
  ],
  [
    "let_const_shadow_and_capture",
    "let value=1; var saved; { let value=3; const delta=4; saved=()=>value+delta; } return \"\"+(saved()+value);",
    "8"
  ],
  [
    "destructured_block_bindings",
    "let value=1; var read; { const {left:a,right:[b=4]}={left:3,right:[]}; let [c]=[5]; read=()=>a+b+c; } return \"\"+(read()+value);",
    "13"
  ],
  [
    "sibling_block_captures",
    "var fs=[]; {let n=1; fs.push(()=>n); n=3;} {let n=9; fs.push(()=>n);} return \"\"+(fs[0]()*10+fs[1]());",
    "39"
  ],
  [
    "annex_b_block_function_hoists_and_shadows_lexical",
    "let name=5; var read; { read=name(); function name(){return 7;} } return \"\"+(read*10+name);",
    "75"
  ],
  [
    "strict_block_function_hoists_and_shadows_lexical",
    "\"use strict\"; let name=5; var read; { read=name(); function name(){return 7;} } return \"\"+(read*10+name);",
    "75"
  ],
  [
    "for_of_iteration_binding_captures",
    "var fs=[]; for(let i of [1,2,3]){fs.push(()=>i);} return \"\"+(fs[0]()*100+fs[1]()*10+fs[2]());",
    "123"
  ],
  [
    "for_body_binding_captures",
    "var fs=[]; for(var i=0;i<3;i++){let captured=i; {fs.push(()=>captured);}} return \"\"+(fs[0]()*100+fs[1]()*10+fs[2]());",
    "12"
  ],
  [
    "catch_binding_and_finally_parent",
    "let x=1; var read; try{{throw 4;}}catch(x){{read=()=>x;}}finally{{x+=2;}} return \"\"+(read()*10+x);",
    "43"
  ],
  [
    "break_continue_nested_binding_free_blocks",
    "let sum=0; outer:for(let i=0;i<4;i++){if(i===1){continue;} for(let j=0;j<3;j++){if(j===2){break;} sum+=i+j;}} return \"\"+sum;",
    "13"
  ],
  [
    "sloppy_direct_eval_var_and_lexicals",
    "var x=1; {eval(\"var x=7; let hidden=9;\");} return x+\":\"+typeof hidden;",
    "7:undefined"
  ],
  [
    "strict_direct_eval_lexicals",
    "\"use strict\"; let x=1; {eval(\"let x=9; const hidden=2; x+hidden;\");} return x+\":\"+typeof hidden;",
    "1:undefined"
  ],
  [
    "direct_eval_nested_in_argument",
    "var x=1; {Math.max(0,eval(\"x=8; x;\"));} return \"\"+x;",
    "8"
  ],
  [
    "shadowed_eval_call",
    "let eval=(s)=>s.length; {eval(\"abc\");} return \"\"+eval(\"abcd\");",
    "4"
  ],
  [
    "nested_function_eval_uses_own_environment",
    "var f; {f=function(){var n=1; eval(\"n=3;\"); return n;};} return \"\"+f();",
    "3"
  ],
  [
    "switch_keeps_its_binding_owner",
    "let x=1; var saved; switch(2){case 2: let x=4; saved=()=>x; break;} return \"\"+(saved()+x);",
    "5"
  ],
  [
    "do_while_binding_free_blocks",
    "var n=0; do{{n++;}}while(n<3); return \"\"+n;",
    "3"
  ]
];
const nonce = String(process.pid);
for (const row of cases) {
  const run = new Function(row[1] + "\n// runtime source " + nonce);
  const actual = run();
  if (typeof actual !== "string" || actual !== row[2]) {
    throw new Error(row[0] + ": expected " + row[2] + ", got " + actual);
  }
  console.log(row[0], actual);
}
