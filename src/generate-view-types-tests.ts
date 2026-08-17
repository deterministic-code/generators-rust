import { createGenerator } from "./generate-view-tests.ts";
import { makeViewGenerate } from "@deterministic-code/generator-sdk/codegen/lib/view-generate-config";

export const generate = makeViewGenerate(
  createGenerator,
  "view_types_tests",
  "rust",
);
