import { fill } from "./common/fill.ts";
import type { GenerateContext } from "./common/generate-context.ts";
import { content, patch, type GenerateEntry } from "./common/generate-entry.ts";
import {
  cargoToml,
  dockerComposeYml,
  dockerfile,
  entrypointSh,
  envFile,
  gitignore,
  libRs,
  mainRs,
} from "./resources/backend-app.ts";

const DEFAULT_APP_NAME = "generated-app";

const crateIdentFromAppName = (appName: string): string => {
  const ident = appName
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  const base = ident || "generated_app";
  return /^[0-9]/.test(base) ? `app_${base}` : base;
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const crateIdent = crateIdentFromAppName(
    ctx.settings.application_name || DEFAULT_APP_NAME,
  );
  const named = { crateIdent };
  return [
    content("src/main.rs", fill(mainRs, named)),
    patch("src/lib.rs", libRs),
    patch("Cargo.toml", fill(cargoToml, named)),
    patch("Dockerfile", fill(dockerfile, named)),
    patch("scripts/entrypoint.sh", entrypointSh),
    patch("docker-compose.yml", dockerComposeYml),
    patch(".env", envFile),
    patch(".env.example", envFile),
    patch(".gitignore", gitignore),
    patch(".dockerignore", "target", "DOCKERIGNORE_RUST"),
  ];
};
