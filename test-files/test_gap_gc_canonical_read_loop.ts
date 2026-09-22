function run(n: any, O: any) {
    let h=0;
    let garbage:any=[];
    for(let k=0;k<n;k++) {
        garbage=[k];
        h+=O.a+O.b;
    }
    console.log(h,garbage[0]);
}
// Heap allocations and copying polls execute while the single guard is live.
run(2000,{a:1,b:2});
