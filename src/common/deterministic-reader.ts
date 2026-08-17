import { access, readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import {
  DATASOURCE_TYPES_YAML,
  parseDatasourceTypes,
  type DatasourceType,
} from "./parse-datasource-types.ts";

/** Host-supplied access to the deterministic YAML tree (disk, memory, zip, …). */
export type IDeterministicReader = {
  read: (name: string) => Promise<string>;
  exists: (name: string) => Promise<boolean>;
  loadDatasourceTypes: (idType: string) => Promise<DatasourceType[]>;
};

type Source = {
  read: (name: string) => Promise<string>;
  exists: (name: string) => Promise<boolean>;
};

export class DeterministicReader implements IDeterministicReader {
  #source: Source;

  constructor(source: Source) {
    this.#source = source;
  }

  read(name: string): Promise<string> {
    return this.#source.read(name);
  }

  exists(name: string): Promise<boolean> {
    return this.#source.exists(name);
  }

  async loadDatasourceTypes(idType: string): Promise<DatasourceType[]> {
    return parseDatasourceTypes({
      yaml: await this.read(DATASOURCE_TYPES_YAML),
      idType,
    });
  }
}

export const memoryReader = (
  files: Record<string, string>,
): IDeterministicReader =>
  new DeterministicReader({
    read: async (name) => {
      if (!(name in files)) {
        throw new Error(`deterministic reader: missing ${name}`);
      }
      return files[name];
    },
    exists: async (name) => name in files,
  });

export const fileReader = (root: string): IDeterministicReader => {
  const dir = resolve(root);
  return new DeterministicReader({
    read: async (name) => {
      try {
        return await readFile(join(dir, name), "utf8");
      } catch {
        throw new Error(`deterministic reader: missing ${name} in ${dir}`);
      }
    },
    exists: async (name) => {
      try {
        await access(join(dir, name));
        return true;
      } catch {
        return false;
      }
    },
  });
};
