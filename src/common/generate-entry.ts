export type GenerateEntry =
  | { kind: "content"; filename: string; contents: string }
  | { kind: "patch"; filename: string; content: string; section?: string };

export const content = (
  filename: string,
  contents: string,
): GenerateEntry => ({
  kind: "content",
  filename,
  contents,
});

export const patch = (
  filename: string,
  fileContent: string,
  section?: string,
): GenerateEntry =>
  section
    ? { kind: "patch", filename, content: fileContent, section }
    : { kind: "patch", filename, content: fileContent };
