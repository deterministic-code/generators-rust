export type CommentStyle = "none" | "simple" | "description";

export const commentStyle = (value: string | undefined): CommentStyle => {
  if (value === "none" || value === "description") return value;
  return "simple";
};

export const renderDocComment = (args: {
  style: CommentStyle;
  summary: string;
  lines: [string, string, string];
  language: "typescript" | "rust" | "csharp";
}): string => {
  if (args.style === "none") return "";
  if (args.style === "simple") {
    if (args.language === "rust") return `/// ${args.summary}\n`;
    return `/** ${args.summary} */\n`;
  }
  if (args.language === "rust") {
    return [args.summary, ...args.lines].map((l) => `/// ${l}`).join("\n") + "\n";
  }
  const body = [args.summary, ...args.lines].map((l) => ` * ${l}`).join("\n");
  return `/**\n${body}\n */\n`;
};
