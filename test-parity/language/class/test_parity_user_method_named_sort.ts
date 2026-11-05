class Sorter {
  sort(list: any[]) {
    return list.sort((a: any, b: any) => (a < b ? -1 : a > b ? 1 : 0));
  }
}
const s = new Sorter();
console.log(JSON.stringify(s.sort(["1.2.0", "1.0.1", "1.0.0", "2.0.0"])));
