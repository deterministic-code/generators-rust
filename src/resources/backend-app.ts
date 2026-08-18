import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(new URL(`../templates/create-backend-app/${rel}`, import.meta.url), "utf8");

export const [
  mainRs,
  libRs,
  cargoToml,
  dockerfile,
  entrypointSh,
  dockerComposeYml,
  envFile,
  gitignore,
] = await Promise.all([
  resource("rust/chunks/main.rs"),
  resource("rust/chunks/lib.rs"),
  resource("Cargo.toml.tmpl"),
  resource("Dockerfile.tmpl"),
  resource("entrypoint.sh"),
  resource("docker-compose.yml.tmpl"),
  resource("env.tmpl"),
  resource("gitignore.tmpl"),
]);
