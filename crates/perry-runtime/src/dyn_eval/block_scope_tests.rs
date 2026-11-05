//! Scope semantics exercised through the runtime interpreter, not AOT bodies.
use super::*;

#[test]
fn binding_free_loops() {
    let function = dyn_fn(&[r#"var sum=0; for(var i=0;i<8;i++){if(i&1){sum+=i;}} return ""+sum;"#]);
    assert_eq!(as_str(call(function, &[])), "16");
}

#[test]
fn binding_free_parent_lexical() {
    let function = dyn_fn(&[r#"let n=3; { { n+=2; } } return ""+n;"#]);
    assert_eq!(as_str(call(function, &[])), "5");
}

#[test]
fn var_hoists_through_block() {
    let function = dyn_fn(&[
        r#"var total=0; { total+=typeof later==="undefined"?1:100; var later=4; } return ""+(total+later);"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "5");
}

#[test]
fn let_const_shadow_and_capture() {
    let function = dyn_fn(&[
        r#"let value=1; var saved; { let value=3; const delta=4; saved=()=>value+delta; } return ""+(saved()+value);"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "8");
}

#[test]
fn destructured_block_bindings() {
    let function = dyn_fn(&[
        r#"let value=1; var read; { const {left:a,right:[b=4]}={left:3,right:[]}; let [c]=[5]; read=()=>a+b+c; } return ""+(read()+value);"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "13");
}

#[test]
fn sibling_block_captures() {
    let function = dyn_fn(&[
        r#"var fs=[]; {let n=1; fs.push(()=>n); n=3;} {let n=9; fs.push(()=>n);} return ""+(fs[0]()*10+fs[1]());"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "39");
}

#[test]
fn annex_b_block_function_hoists_and_shadows_lexical() {
    let function = dyn_fn(&[
        r#"let name=5; var read; { read=name(); function name(){return 7;} } return ""+(read*10+name);"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "75");
}

#[test]
fn strict_block_function_hoists_and_shadows_lexical() {
    let function = dyn_fn(&[
        r#""use strict"; let name=5; var read; { read=name(); function name(){return 7;} } return ""+(read*10+name);"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "75");
}

#[test]
fn for_of_iteration_binding_captures() {
    let function = dyn_fn(&[
        r#"var fs=[]; for(let i of [1,2,3]){fs.push(()=>i);} return ""+(fs[0]()*100+fs[1]()*10+fs[2]());"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "123");
}

#[test]
fn for_body_binding_captures() {
    let function = dyn_fn(&[
        r#"var fs=[]; for(var i=0;i<3;i++){let captured=i; {fs.push(()=>captured);}} return ""+(fs[0]()*100+fs[1]()*10+fs[2]());"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "12");
}

#[test]
fn catch_binding_and_finally_parent() {
    let function = dyn_fn(&[
        r#"let x=1; var read; try{{throw 4;}}catch(x){{read=()=>x;}}finally{{x+=2;}} return ""+(read()*10+x);"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "43");
}

#[test]
fn break_continue_nested_binding_free_blocks() {
    let function = dyn_fn(&[
        r#"let sum=0; outer:for(let i=0;i<4;i++){if(i===1){continue;} for(let j=0;j<3;j++){if(j===2){break;} sum+=i+j;}} return ""+sum;"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "13");
}

#[test]
fn sloppy_direct_eval_var_and_lexicals() {
    let function =
        dyn_fn(&[r#"var x=1; {eval("var x=7; let hidden=9;");} return x+":"+typeof hidden;"#]);
    assert_eq!(as_str(call(function, &[])), "7:undefined");
}

#[test]
fn strict_direct_eval_lexicals() {
    let function = dyn_fn(&[
        r#""use strict"; let x=1; {eval("let x=9; const hidden=2; x+hidden;");} return x+":"+typeof hidden;"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "1:undefined");
}

#[test]
fn direct_eval_nested_in_argument() {
    let function = dyn_fn(&[r#"var x=1; {Math.max(0,eval("x=8; x;"));} return ""+x;"#]);
    assert_eq!(as_str(call(function, &[])), "8");
}

#[test]
fn shadowed_eval_call() {
    let function = dyn_fn(&[r#"let eval=(s)=>s.length; {eval("abc");} return ""+eval("abcd");"#]);
    assert_eq!(as_str(call(function, &[])), "4");
}

#[test]
fn nested_function_eval_uses_own_environment() {
    let function =
        dyn_fn(&[r#"var f; {f=function(){var n=1; eval("n=3;"); return n;};} return ""+f();"#]);
    assert_eq!(as_str(call(function, &[])), "3");
}

#[test]
fn switch_keeps_its_binding_owner() {
    let function = dyn_fn(&[
        r#"let x=1; var saved; switch(2){case 2: let x=4; saved=()=>x; break;} return ""+(saved()+x);"#,
    ]);
    assert_eq!(as_str(call(function, &[])), "5");
}

#[test]
fn do_while_binding_free_blocks() {
    let function = dyn_fn(&[r#"var n=0; do{{n++;}}while(n<3); return ""+n;"#]);
    assert_eq!(as_str(call(function, &[])), "3");
}
