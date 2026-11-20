// Repeated and distinct literals, closure captures, site globals, and late bindings.
let total = 0;
const callbacks: ((x: any) => string)[] = [
  (x: any) => { total += x.value + 0; const a = [x]; a.push(x); return "shared:" + x.name + ":0:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 1; const a = [x]; a.push(x); return "shared:" + x.name + ":1:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 2; const a = [x]; a.push(x); return "shared:" + x.name + ":2:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 3; const a = [x]; a.push(x); return "shared:" + x.name + ":3:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 4; const a = [x]; a.push(x); return "shared:" + x.name + ":4:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 5; const a = [x]; a.push(x); return "shared:" + x.name + ":5:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 6; const a = [x]; a.push(x); return "shared:" + x.name + ":6:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 7; const a = [x]; a.push(x); return "shared:" + x.name + ":7:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 8; const a = [x]; a.push(x); return "shared:" + x.name + ":8:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 9; const a = [x]; a.push(x); return "shared:" + x.name + ":9:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 10; const a = [x]; a.push(x); return "shared:" + x.name + ":10:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 11; const a = [x]; a.push(x); return "shared:" + x.name + ":11:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 12; const a = [x]; a.push(x); return "shared:" + x.name + ":12:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 13; const a = [x]; a.push(x); return "shared:" + x.name + ":13:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 14; const a = [x]; a.push(x); return "shared:" + x.name + ":14:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 15; const a = [x]; a.push(x); return "shared:" + x.name + ":15:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 16; const a = [x]; a.push(x); return "shared:" + x.name + ":16:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 17; const a = [x]; a.push(x); return "shared:" + x.name + ":17:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 18; const a = [x]; a.push(x); return "shared:" + x.name + ":18:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 19; const a = [x]; a.push(x); return "shared:" + x.name + ":19:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 20; const a = [x]; a.push(x); return "shared:" + x.name + ":20:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 21; const a = [x]; a.push(x); return "shared:" + x.name + ":21:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 22; const a = [x]; a.push(x); return "shared:" + x.name + ":22:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 23; const a = [x]; a.push(x); return "shared:" + x.name + ":23:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 24; const a = [x]; a.push(x); return "shared:" + x.name + ":24:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 25; const a = [x]; a.push(x); return "shared:" + x.name + ":25:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 26; const a = [x]; a.push(x); return "shared:" + x.name + ":26:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 27; const a = [x]; a.push(x); return "shared:" + x.name + ":27:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 28; const a = [x]; a.push(x); return "shared:" + x.name + ":28:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 29; const a = [x]; a.push(x); return "shared:" + x.name + ":29:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 30; const a = [x]; a.push(x); return "shared:" + x.name + ":30:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 31; const a = [x]; a.push(x); return "shared:" + x.name + ":31:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 32; const a = [x]; a.push(x); return "shared:" + x.name + ":32:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 33; const a = [x]; a.push(x); return "shared:" + x.name + ":33:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 34; const a = [x]; a.push(x); return "shared:" + x.name + ":34:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 35; const a = [x]; a.push(x); return "shared:" + x.name + ":35:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 36; const a = [x]; a.push(x); return "shared:" + x.name + ":36:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 37; const a = [x]; a.push(x); return "shared:" + x.name + ":37:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 38; const a = [x]; a.push(x); return "shared:" + x.name + ":38:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 39; const a = [x]; a.push(x); return "shared:" + x.name + ":39:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 40; const a = [x]; a.push(x); return "shared:" + x.name + ":40:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 41; const a = [x]; a.push(x); return "shared:" + x.name + ":41:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 42; const a = [x]; a.push(x); return "shared:" + x.name + ":42:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 43; const a = [x]; a.push(x); return "shared:" + x.name + ":43:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 44; const a = [x]; a.push(x); return "shared:" + x.name + ":44:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 45; const a = [x]; a.push(x); return "shared:" + x.name + ":45:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 46; const a = [x]; a.push(x); return "shared:" + x.name + ":46:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 47; const a = [x]; a.push(x); return "shared:" + x.name + ":47:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 48; const a = [x]; a.push(x); return "shared:" + x.name + ":48:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 49; const a = [x]; a.push(x); return "shared:" + x.name + ":49:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 50; const a = [x]; a.push(x); return "shared:" + x.name + ":50:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 51; const a = [x]; a.push(x); return "shared:" + x.name + ":51:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 52; const a = [x]; a.push(x); return "shared:" + x.name + ":52:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 53; const a = [x]; a.push(x); return "shared:" + x.name + ":53:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 54; const a = [x]; a.push(x); return "shared:" + x.name + ":54:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 55; const a = [x]; a.push(x); return "shared:" + x.name + ":55:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 56; const a = [x]; a.push(x); return "shared:" + x.name + ":56:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 57; const a = [x]; a.push(x); return "shared:" + x.name + ":57:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 58; const a = [x]; a.push(x); return "shared:" + x.name + ":58:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 59; const a = [x]; a.push(x); return "shared:" + x.name + ":59:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 60; const a = [x]; a.push(x); return "shared:" + x.name + ":60:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 61; const a = [x]; a.push(x); return "shared:" + x.name + ":61:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 62; const a = [x]; a.push(x); return "shared:" + x.name + ":62:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 63; const a = [x]; a.push(x); return "shared:" + x.name + ":63:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 64; const a = [x]; a.push(x); return "shared:" + x.name + ":64:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 65; const a = [x]; a.push(x); return "shared:" + x.name + ":65:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 66; const a = [x]; a.push(x); return "shared:" + x.name + ":66:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 67; const a = [x]; a.push(x); return "shared:" + x.name + ":67:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 68; const a = [x]; a.push(x); return "shared:" + x.name + ":68:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 69; const a = [x]; a.push(x); return "shared:" + x.name + ":69:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 70; const a = [x]; a.push(x); return "shared:" + x.name + ":70:" + /ok/.test(x.name) + ":" + "\ud800".length; },
  (x: any) => { total += x.value + 71; const a = [x]; a.push(x); return "shared:" + x.name + ":71:" + /ok/.test(x.name) + ":" + "\ud800".length; },
];
function fn0(x: any): string { return x.name + ":shared:0:" + x.value; }
function fn1(x: any): string { return x.name + ":shared:1:" + x.value; }
function fn2(x: any): string { return x.name + ":shared:2:" + x.value; }
function fn3(x: any): string { return x.name + ":shared:3:" + x.value; }
function fn4(x: any): string { return x.name + ":shared:4:" + x.value; }
function fn5(x: any): string { return x.name + ":shared:5:" + x.value; }
function fn6(x: any): string { return x.name + ":shared:6:" + x.value; }
function fn7(x: any): string { return x.name + ":shared:7:" + x.value; }
function fn8(x: any): string { return x.name + ":shared:8:" + x.value; }
function fn9(x: any): string { return x.name + ":shared:9:" + x.value; }
function fn10(x: any): string { return x.name + ":shared:10:" + x.value; }
function fn11(x: any): string { return x.name + ":shared:11:" + x.value; }
function fn12(x: any): string { return x.name + ":shared:12:" + x.value; }
function fn13(x: any): string { return x.name + ":shared:13:" + x.value; }
function fn14(x: any): string { return x.name + ":shared:14:" + x.value; }
function fn15(x: any): string { return x.name + ":shared:15:" + x.value; }
function fn16(x: any): string { return x.name + ":shared:16:" + x.value; }
function fn17(x: any): string { return x.name + ":shared:17:" + x.value; }
function fn18(x: any): string { return x.name + ":shared:18:" + x.value; }
function fn19(x: any): string { return x.name + ":shared:19:" + x.value; }
function fn20(x: any): string { return x.name + ":shared:20:" + x.value; }
function fn21(x: any): string { return x.name + ":shared:21:" + x.value; }
function fn22(x: any): string { return x.name + ":shared:22:" + x.value; }
function fn23(x: any): string { return x.name + ":shared:23:" + x.value; }
function fn24(x: any): string { return x.name + ":shared:24:" + x.value; }
function fn25(x: any): string { return x.name + ":shared:25:" + x.value; }
function fn26(x: any): string { return x.name + ":shared:26:" + x.value; }
function fn27(x: any): string { return x.name + ":shared:27:" + x.value; }
function fn28(x: any): string { return x.name + ":shared:28:" + x.value; }
function fn29(x: any): string { return x.name + ":shared:29:" + x.value; }
function fn30(x: any): string { return x.name + ":shared:30:" + x.value; }
function fn31(x: any): string { return x.name + ":shared:31:" + x.value; }
function fn32(x: any): string { return x.name + ":shared:32:" + x.value; }
function fn33(x: any): string { return x.name + ":shared:33:" + x.value; }
function fn34(x: any): string { return x.name + ":shared:34:" + x.value; }
function fn35(x: any): string { return x.name + ":shared:35:" + x.value; }
function fn36(x: any): string { return x.name + ":shared:36:" + x.value; }
function fn37(x: any): string { return x.name + ":shared:37:" + x.value; }
function fn38(x: any): string { return x.name + ":shared:38:" + x.value; }
function fn39(x: any): string { return x.name + ":shared:39:" + x.value; }
function fn40(x: any): string { return x.name + ":shared:40:" + x.value; }
function fn41(x: any): string { return x.name + ":shared:41:" + x.value; }
function fn42(x: any): string { return x.name + ":shared:42:" + x.value; }
function fn43(x: any): string { return x.name + ":shared:43:" + x.value; }
function fn44(x: any): string { return x.name + ":shared:44:" + x.value; }
function fn45(x: any): string { return x.name + ":shared:45:" + x.value; }
function fn46(x: any): string { return x.name + ":shared:46:" + x.value; }
function fn47(x: any): string { return x.name + ":shared:47:" + x.value; }
function fn48(x: any): string { return x.name + ":shared:48:" + x.value; }
function fn49(x: any): string { return x.name + ":shared:49:" + x.value; }
function fn50(x: any): string { return x.name + ":shared:50:" + x.value; }
function fn51(x: any): string { return x.name + ":shared:51:" + x.value; }
function fn52(x: any): string { return x.name + ":shared:52:" + x.value; }
function fn53(x: any): string { return x.name + ":shared:53:" + x.value; }
function fn54(x: any): string { return x.name + ":shared:54:" + x.value; }
function fn55(x: any): string { return x.name + ":shared:55:" + x.value; }
function fn56(x: any): string { return x.name + ":shared:56:" + x.value; }
function fn57(x: any): string { return x.name + ":shared:57:" + x.value; }
function fn58(x: any): string { return x.name + ":shared:58:" + x.value; }
function fn59(x: any): string { return x.name + ":shared:59:" + x.value; }
function fn60(x: any): string { return x.name + ":shared:60:" + x.value; }
function fn61(x: any): string { return x.name + ":shared:61:" + x.value; }
function fn62(x: any): string { return x.name + ":shared:62:" + x.value; }
function fn63(x: any): string { return x.name + ":shared:63:" + x.value; }
function fn64(x: any): string { return x.name + ":shared:64:" + x.value; }
function fn65(x: any): string { return x.name + ":shared:65:" + x.value; }
function fn66(x: any): string { return x.name + ":shared:66:" + x.value; }
function fn67(x: any): string { return x.name + ":shared:67:" + x.value; }
function fn68(x: any): string { return x.name + ":shared:68:" + x.value; }
function fn69(x: any): string { return x.name + ":shared:69:" + x.value; }
function fn70(x: any): string { return x.name + ":shared:70:" + x.value; }
function fn71(x: any): string { return x.name + ":shared:71:" + x.value; }
const input = { name: "ok", value: 3 };
let checksum = 0;
for (let i = 0; i < callbacks.length; i++) { checksum += callbacks[i](input).length; }
console.log(total, checksum, callbacks.length, fn0(input), fn71(input));
const encodings = ["\ud800", "wtf8:\\xed\\xa0\\x80"];
console.log(encodings[0].length, encodings[1].length, encodings[0].charCodeAt(0));
