export const fill = (
  text: string,
  tokens: Record<string, string>,
): string =>
  text.replace(/\{\{(\w+)\}\}/g, (_match: string, key: string) => {
    if (!(key in tokens)) {
      throw new Error(`Unresolved placeholder: {{${key}}}`);
    }
    return tokens[key];
  });
