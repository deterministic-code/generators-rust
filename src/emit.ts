import {
  fromSettings,
  type ISettings,
} from "@deterministic-code/generators-common/settings";
import { createCasing, type PackCasing } from "./common/default-casing.ts";
import {
  createImportGenerator,
  type RustImportGenerator,
} from "./import-generator.ts";

/** Settings plus a pack import generator created once; lanes use `this.imports`. */
export class Emit {
  readonly settings: ISettings;
  readonly casing: PackCasing;
  readonly imports: RustImportGenerator;

  constructor(raw: Record<string, string>, basePath = ".") {
    this.settings = fromSettings(raw);
    this.casing = createCasing(raw);
    this.imports = createImportGenerator(basePath, raw);
  }
}
