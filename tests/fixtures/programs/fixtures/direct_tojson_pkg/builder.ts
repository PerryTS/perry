export class ImportedCommandBuilder {
  value = "deploy";

  setValue(value: string): this {
    this.value = value;
    return this;
  }

  toJSON(): any {
    return { name: this.value };
  }
}
