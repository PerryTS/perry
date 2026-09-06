from pathlib import Path
import subprocess
import tempfile
root=Path(__file__).resolve().parents[4];art=Path(tempfile.mkdtemp(prefix='perry-tape-diff-'))
path='crates/perry-runtime/src/json_tape.rs'
old=subprocess.check_output(['git','show','f4221890b1d814299e2a2a243f0aefa7cdff7366:'+path],cwd=root,text=True);new=(root/path).read_text()
def module(name,source):
    types=source[source.index('#[derive(Debug, Clone, Copy)]'):source.index('struct TapeScratch')]
    body=source[source.index('fn build_tape_into('):source.index('/// Build a tape using thread-local')]
    return 'mod '+name+' {\n'+types+body+'\npub fn check(b: &[u8]) -> Option<Vec<(u32,u8,u32)>> { let mut e=Vec::new(); let mut s=Vec::new(); if build_tape_into(b,&mut e,&mut s) { Some(e.iter().map(|x|(x.offset,x.kind,x.link)).collect()) } else { None } }\n}\n'
s='mod json { #[path = "'+str(root/'crates/perry-runtime/src/json/simd.rs')+'"] pub mod simd; }\n'+module('before',old)+module('after',new)
s+=r'''
fn check(b:&[u8],n:&mut usize) { assert_eq!(before::check(b),after::check(b),"{:?}",b); *n+=1; }
fn main() {
    let mut count=0;
    let tokens=["0","-0","123456789012345678901234567890","0.01234567890123456789","-1234.125e+12345","true","false","null",r#""\u1234\n\"\\東京🙂""#];
    let mut corpus=Vec::new();
    for len in [0,1,7,8,9,15,16,17,31,32,33,63,64,65,127,128,129,1024] {
        for token in tokens { corpus.push(format!(r#"["{}",{},{{"next":[true,false,null]}}]"#,"a".repeat(len),token).into_bytes()); }
    }
    for b in &corpus { check(b,&mut count); for end in 0..b.len() {check(&b[..end],&mut count);} }
    let mut rng=0xa343dd79bb93ed49u64;
    for i in 0..300000 {
        rng^=rng<<13; rng^=rng>>7; rng^=rng<<17;
        let mut b=corpus[i%corpus.len()].clone();
        let at=rng as usize%b.len();
        b[at]=(rng>>32) as u8;
        if i%3==0 && at+1<b.len(){ b[at+1]=(rng>>40) as u8; }
        if i%7==0 {b.insert(at,(rng>>48) as u8);}
        if i%11==0 {b.remove(at);}
        check(&b,&mut count);
    }
    println!("baseline/candidate tape agreement: {count} inputs (validity and every token offset, kind, link)");
}
'''
(art/'tape_diff.rs').write_text(s)
subprocess.run(['rustc','--edition=2021','-C','opt-level=3','-A','dead_code',str(art/'tape_diff.rs'),'-o',str(art/'tape_diff')],cwd=root,check=True)
subprocess.run([str(art/'tape_diff')],check=True)
