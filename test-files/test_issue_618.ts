function sql(strings: any, ...params: any[]) {
  return { kind: 'sql', s: strings };
}

((sql2: any) => {
  sql2.identifier = function (v: any) {
    return { kind: 'name', v };
  };
})(sql);

console.log('sql.identifier:', typeof (sql as any).identifier);
console.log('sql.identifier("foo"):', (sql as any).identifier("foo"));
