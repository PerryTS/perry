export class Query {
  params(limit: number, search: string) {
    const params: Record<string, string> = {};
    if (limit) params.limit = String(limit);
    if (search) params.search = search;
    return params;
  }
}
