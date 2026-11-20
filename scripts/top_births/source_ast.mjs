const ts = await import(process.env.PERRY_BIRTH_TYPESCRIPT_MODULE || 'typescript');
import fs from 'node:fs';
const suffix=s=>s.endsWith('/')?s:s+'/'; const R=suffix(process.env.PERRY_BIRTH_OUTPUT || '.'), root=suffix(process.env.PERRY_BIRTH_SOURCE_ROOT || '/root/hwp-move/score/p/tssrc/');
const req=JSON.parse(fs.readFileSync(R+'source-request.json','utf8'));
const globals=new Set(req.globals), modules=new Set(req.modules), methods=new Set(req.methods);
const out={expressions:{},entries:{},globals:{},modules:{},methods:{}};
const clean=s=>s.replace(/^node:/,'');
const add=(dict,key,value)=>{(dict[key]??=[]).push(value)};
for(const file of req.files){
 const text=fs.readFileSync(file,'utf8'), sf=ts.createSourceFile(file,text,ts.ScriptTarget.Latest,true);
 const bytes=new Uint32Array(text.length+1);let u=0,b=0;
 for(const ch of text){const cp=ch.codePointAt(0),n=ch.length,k=cp<128?1:cp<2048?2:cp<65536?3:4;bytes[u]=b;if(n===2)bytes[u+1]=b;u+=n;b+=k;bytes[u]=b;}
 const rel=file.startsWith(root)?file.slice(root.length):file;
 const pos=n=>{const a=n.getStart(sf),z=n.end;return {source:rel+':'+(sf.getLineAndCharacterOfPosition(a).line+1),expression:text.slice(a,z).replace(/\s+/g,' ').slice(0,180),start:bytes[a]+1,end:bytes[z]+1}};
 const key=p=>p.source+'\t'+p.expression;
 const scopes=new WeakMap(), all=[];let seq=0;
 const mk=(p,fn=false)=>({p,fn,id:seq++,bindings:new Map()});const top=mk(null,true);
 const declare=(scope,name,record)=>{if(ts.isIdentifier(name))scope.bindings.set(name.text,record);else if(name.elements)for(const e of name.elements)if(e.name)declare(scope,e.name,{kind:'unknown'});};
 const ambient=n=>n.modifiers?.some(m=>m.kind===ts.SyntaxKind.DeclareKeyword);
 function build(n,scope){
  if(ts.isInterfaceDeclaration(n)||ts.isTypeAliasDeclaration(n)||ts.isTypeNode(n)&&n.kind!==ts.SyntaxKind.ExpressionWithTypeArguments)return;
  if(ts.isFunctionDeclaration(n)&&n.name)declare(scope,n.name,!n.body&&globals.has(n.name.text)?{kind:'global',name:n.name.text}:{kind:'unknown'});
  let here=scope;
  if(n!==sf&&(ts.isFunctionLike(n)||ts.isBlock(n)||ts.isCatchClause(n)))here=mk(scope,ts.isFunctionLike(n));
  scopes.set(n,here);all.push(n);
  if(ts.isParameter(n))declare(here,n.name,{kind:'unknown'});
  if(ts.isVariableDeclaration(n)){
   let dest=here;const list=n.parent;
   if(ts.isVariableDeclarationList(list)&&!(list.flags&ts.NodeFlags.BlockScoped))while(dest.p&&!dest.fn)dest=dest.p;
   const stmt=list?.parent;const rec=ambient(stmt)&&ts.isIdentifier(n.name)&&globals.has(n.name.text)?{kind:'global',name:n.name.text}:{kind:'init',node:n.initializer,scope:here};
   declare(dest,n.name,rec);
  }
  if(ts.isImportDeclaration(n)&&ts.isStringLiteral(n.moduleSpecifier)){
   const mod=clean(n.moduleSpecifier.text),c=n.importClause;
   if(c&&!c.isTypeOnly){if(c.name)declare(here,c.name,{kind:'module',name:mod});const x=c.namedBindings;
    if(x&&ts.isNamespaceImport(x))declare(here,x.name,{kind:'module',name:mod});
    else if(x)for(const e of x.elements)declare(here,e.name,e.isTypeOnly?{kind:'unknown'}:{kind:'method',name:mod+'.'+(e.propertyName??e.name).text});
   }
  }
  ts.forEachChild(n,c=>build(c,here));
 }
 build(sf,top);
 function binding(scope,name){for(let s=scope;s;s=s.p)if(s.bindings.has(name))return s.bindings.get(name);return null;}
 function unwrap(n){while(n&&(ts.isAsExpression(n)||ts.isTypeAssertionExpression(n)||ts.isParenthesizedExpression(n)||ts.isNonNullExpression(n)||ts.isAwaitExpression(n)))n=n.expression;return n;}
 function resolve(node,scope,seen=new Set()){
  const n=unwrap(node);if(!n)return null;
  if(ts.isIdentifier(n)){
   const rec=binding(scope,n.text);
   if(!rec)return globals.has(n.text)?{kind:'global',name:n.text}:null;
   if(rec.kind!=='init')return rec;
   if(seen.has(rec))return null;seen.add(rec);return resolve(rec.node,rec.scope,seen);
  }
  if(ts.isCallExpression(n)&&ts.isIdentifier(n.expression)&&n.expression.text==='require'&&n.arguments.length&&ts.isStringLiteral(n.arguments[0]))return {kind:'module',name:clean(n.arguments[0].text)};
  if(ts.isPropertyAccessExpression(n)){const obj=resolve(n.expression,scope,seen);if(obj?.kind==='module'||obj?.kind==='object')return {kind:'method',name:obj.name+'.'+n.name.text};}
  if(ts.isCallExpression(n)||ts.isNewExpression(n)){const callee=resolve(n.expression,scope,seen);if(callee?.kind==='method')return {kind:'object',name:callee.name.split('.')[0]};}
  return null;
 }
 function valueIdentifier(n){
  const p=n.parent;if(!p)return false;
  if(ts.isShorthandPropertyAssignment(p)&&p.name===n)return true;
  if(p.name===n)return false;
  return ts.isInExpressionContext(n);
 }
 for(const n of all){
  if(n.parent&&((typeof ts.isExpression==='function'&&ts.isExpression(n)&&(!ts.isIdentifier(n)||valueIdentifier(n)))||(ts.isShorthandPropertyAssignment(n.parent)&&n.parent.name===n))){const p=pos(n);out.expressions[key(p)]=true;}
  if(ts.isFunctionLike(n)||ts.isClassDeclaration(n)||ts.isObjectLiteralExpression(n)){const p=pos(n);out.entries[key(p)]=true;}
  if(ts.isIdentifier(n)&&valueIdentifier(n)){
   const p=n.parent,callee=(ts.isCallExpression(p)||ts.isNewExpression(p))&&p.expression===n;
   const v=resolve(n,scopes.get(n));if(!callee&&v?.kind==='global'&&globals.has(v.name))add(out.globals,v.name,pos(n));
  }
  if(ts.isCallExpression(n)||ts.isNewExpression(n)){
   if(ts.isCallExpression(n)&&ts.isIdentifier(n.expression)&&n.expression.text==='require'&&n.arguments.length&&ts.isStringLiteral(n.arguments[0])){
    const name=clean(n.arguments[0].text);if(modules.has(name))add(out.modules,name,pos(n));
   }
   if(ts.isCallExpression(n)&&n.expression.kind===ts.SyntaxKind.ImportKeyword&&n.arguments.length&&ts.isStringLiteral(n.arguments[0])){
    const name=clean(n.arguments[0].text);if(modules.has(name))add(out.modules,name,pos(n));
   }
   const v=resolve(n.expression,scopes.get(n));if(v?.kind==='method'&&methods.has(v.name))add(out.methods,v.name,pos(n));
  }
 }
}
for(const type of ['globals','modules','methods'])for(const [name,rows] of Object.entries(out[type])){const seen=new Set();out[type][name]=rows.filter(r=>{const k=r.source+'\t'+r.expression;if(seen.has(k))return false;seen.add(k);return true;});}
fs.writeFileSync(R+'source-ast.json',JSON.stringify(out));
console.log(JSON.stringify({expressions:Object.keys(out.expressions).length,globals:out.globals,modules:out.modules,methods:out.methods},null,2));
