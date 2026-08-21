import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createImportGenerator } from "./import-generator.ts";

describe("RustImportGenerator", () => {
  it("returns layered files when organize_by_feature is unset", () => {
    const imports = createImportGenerator(".", {});
    assert.equal(imports.datasource("user"), "user.rs");
    assert.equal(imports.view("card_payment"), "card_payment.rs");
    assert.equal(imports.service("user"), "user_service.rs");
    assert.equal(imports.route("user"), "users.rs");
    assert.equal(imports.index("user.rs"), "mod.rs");
    assert.equal(
      imports.spec("", "types/generated/datasource/user.rs"),
      "crate::types::generated::datasource::user",
    );
    assert.equal(
      imports.datasourceQual("user"),
      "crate::types::generated::datasource::User",
    );
    assert.equal(imports.appWiring(), "app_wiring.rs");
  });

  it("nests files under features/ when organize_by_feature is true", () => {
    const imports = createImportGenerator(".", {
      "other.organize_by_feature": "true",
    });
    assert.equal(imports.datasource("user"), "features/user/user.rs");
    assert.equal(
      imports.view("create_card_payment"),
      "features/card_payment/create_card_payment.rs",
    );
    assert.equal(imports.route("user"), "features/user/users_router.rs");
    assert.equal(imports.index("features/user/user.rs"), "");
    assert.equal(
      imports.datasourceQual("user"),
      "crate::features::user::user::User",
    );
    assert.equal(imports.appWiring(), "features/app_wiring.rs");
    assert.equal(
      imports.test("features/user/user.rs", "user"),
      "features/user/__tests__/user_tests.rs",
    );
  });
});
