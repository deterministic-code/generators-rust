export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** First key/value of a single-key YAML map (`{ user: { fields: … } }`). */
export const entryOf = (
  obj: Record<string, unknown>,
): [string, unknown] => {
  const keys = Object.keys(obj);
  return [keys[0], obj[keys[0]]];
};

/** Single-key maps from a YAML list (`- user: …`, `- email: …`). */
export const namedEntries = (
  value: unknown,
): Array<[string, unknown]> =>
  Array.isArray(value)
    ? value.flatMap((item) => {
        if (!isRecord(item)) return [];
        const name = Object.keys(item)[0];
        return name === undefined ? [] : [[name, item[name]]];
      })
    : [];

export const isFiniteInt = (v: unknown): boolean =>
  typeof v === "number" && Number.isFinite(v) && Number.isInteger(v);

export const isFiniteNumber = (v: unknown): boolean =>
  typeof v === "number" && Number.isFinite(v);
