import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-routes/${rel}`, import.meta.url),
    "utf8",
  );

export const [
  readonlyTmpl,
  crudTmpl,
  appWiringTmpl,
  appWiringBodyTmpl,
  byFieldMergeTmpl,
  validatorEmptyTmpl,
  validatorTmpl,
  checkRequiredTypedTmpl,
  checkRequiredTmpl,
  checkOptionalTypedTmpl,
  checkEagerChildTmpl,
  coerceRowTmpl,
] = await Promise.all([
  resource("readonly.rs.tmpl"),
  resource("crud.rs.tmpl"),
  resource("app-wiring.rs.tmpl"),
  resource("app-wiring-body.rs.tmpl"),
  resource("by-field-merge.rs.tmpl"),
  resource("validator-empty.rs.tmpl"),
  resource("validator.rs.tmpl"),
  resource("check-required-typed.rs.tmpl"),
  resource("check-required.rs.tmpl"),
  resource("check-optional-typed.rs.tmpl"),
  resource("check-eager-child.rs.tmpl"),
  resource("coerce-row.rs.tmpl"),
]);
