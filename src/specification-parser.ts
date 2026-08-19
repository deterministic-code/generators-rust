import type { IDeterministicReader } from "@deterministic-code/generators-common/deterministic-reader";
import {
  SpecificationParser as CommonParser,
  rustNaming,
} from "@deterministic-code/generators-common/specification-parser";

export {
  csharpNaming,
  rustNaming,
  typescriptNaming,
  type SpecificationNaming,
} from "@deterministic-code/generators-common/specification-parser";

export * from "@deterministic-code/generators-common/specification";

export class SpecificationParser extends CommonParser {
  constructor(reader?: IDeterministicReader) {
    super(reader, rustNaming);
  }
}
