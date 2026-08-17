/** First key/value of a single-key YAML map (`{ user: { fields: … } }`). */
export const entryOf = (
  obj: Record<string, unknown>,
): [string, unknown] => {
  const keys = Object.keys(obj);
  return [keys[0], obj[keys[0]]];
};

export const isFiniteInt = (v: unknown): boolean =>
  typeof v === "number" && Number.isFinite(v) && Number.isInteger(v);

export const isFiniteNumber = (v: unknown): boolean =>
  typeof v === "number" && Number.isFinite(v);
