import { createGenerator } from "./generate-view-validator.ts";
import { makeViewGenerate } from "@deterministic-code/generator-sdk/codegen/lib/view-generate-config";

export const generate = makeViewGenerate(
  createGenerator,
  "view_type_validators",
  "rust",
);
