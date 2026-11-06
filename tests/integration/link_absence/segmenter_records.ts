// HWS companion: intl_segmenter
// HWS hook: js_runtime_install_intl_segmenter
// HWS prefixes: perry_runtime::intl::segmenter::|perry_runtime::intl::segments_view::
const segmenter=new Intl.Segmenter("en",{granularity:"word"});
const first=Array.from(segmenter.segment("one two"));
for(let i=0;i<20000;i++){const value={payload:"value"+i};if(i===19999)console.log(value.payload)}
const second=Array.from(segmenter.segment("three four"));
console.log(first.map(record=>record.segment+":"+record.index).join("|"));
console.log(second.map(record=>record.segment+":"+record.index).join("|"));
