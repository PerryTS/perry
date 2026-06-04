export type MatrixPayload = {
  label: string;
  value: number;
};

export const matrixValue = 7;

export function render(payload: MatrixPayload): string {
  return payload.label + ":" + (payload.value + matrixValue);
}
