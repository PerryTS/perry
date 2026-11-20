type ColumnConfig = {
  name: string;
  uniqueName?: string;
  autoIncrement: boolean;
};

let constructorCalls = 0;

class Column {
  config: ColumnConfig;

  constructor(table: string, config: ColumnConfig) {
    constructorCalls++;
    if (!config.uniqueName) {
      config.uniqueName = `${table}_${config.name}_unique`;
    }
    this.config = config;
  }
}

export class ColumnWithAutoIncrement extends Column {
  autoIncrement = this.config.autoIncrement;
}

export function getConstructorCalls(): number {
  return constructorCalls;
}
