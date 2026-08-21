import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createImportGenerator } from "./import-generator.ts";

const layered = () => createImportGenerator(".", {});
const byFeature = (extra: Record<string, string> = {}) =>
  createImportGenerator(".", {
    "other.organize_by_feature": "true",
    ...extra,
  });
const flat = (basePath: string, extra: Record<string, string> = {}) =>
  createImportGenerator(basePath, extra);

describe("RustImportGenerator layered (organize_by_feature unset)", () => {
  it("emits identity files, crate specs, and parent-module type quals", () => {
    const imports = layered();
    assert.equal(imports.datasource("user"), "user.rs");
    assert.equal(
      imports.datasourceRel("user"),
      "types/generated/datasource/user.rs",
    );
    assert.equal(
      imports.datasourceQual("user"),
      "crate::types::generated::datasource::User",
    );
    assert.equal(imports.datasourceValidator("user"), "datasourceUserValidator.rs");
    assert.equal(
      imports.datasourceValidatorRel("user"),
      "types/generated/datasource/validators/datasourceUserValidator.rs",
    );
    assert.equal(imports.view("card_payment"), "cardPayment.rs");
    assert.equal(
      imports.viewRel("card_payment"),
      "types/generated/views/cardPayment.rs",
    );
    assert.equal(
      imports.viewQual("card_payment"),
      "crate::types::generated::views::CardPayment",
    );
    assert.equal(imports.viewValidator("card_payment"), "cardPaymentValidator.rs");
    assert.equal(
      imports.viewValidatorRel("user"),
      "types/generated/views/validators/userValidator.rs",
    );
    assert.equal(imports.service("user"), "userService.rs");
    assert.equal(
      imports.serviceRel("user"),
      "services/generated/userService.rs",
    );
    assert.equal(imports.serviceTest("user"), "userServiceTests.rs");
    assert.equal(
      imports.serviceTestRel("user"),
      "services/generated/__tests__/userServiceTests.rs",
    );
    assert.equal(
      imports.serviceIntegrationTest("user"),
      "userServiceIntegrationTests.rs",
    );
    assert.equal(
      imports.serviceIntegrationTestRel("user"),
      "services/generated/__tests__/userServiceIntegrationTests.rs",
    );
    assert.equal(
      imports.serviceCustomRel("user"),
      "services/custom/userService.rs",
    );
    assert.equal(
      imports.serviceUse("user", "UserService"),
      "use crate::services::generated::userService::UserService;",
    );
    assert.equal(imports.route("user"), "users.rs");
    assert.equal(imports.route("card_payment"), "cardPayments.rs");
    assert.equal(imports.routeRel("user"), "routes/generated/users.rs");
    assert.equal(imports.routeModule("user"), "users");
    assert.equal(imports.routeTest("user"), "usersTests.rs");
    assert.equal(imports.index("user.rs"), "mod.rs");
    assert.equal(imports.index("types/user.rs"), "types/mod.rs");
    assert.equal(imports.test("user.rs", "user"), "userTests.rs");
    assert.equal(
      imports.testSpec("user.rs", "user"),
      "crate::user",
    );
    assert.equal(
      imports.spec("", "types/generated/datasource/user.rs"),
      "crate::types::generated::datasource::user",
    );
    assert.equal(imports.spec("", "foo"), "crate::foo");
    assert.equal(imports.appWiring(), "app_wiring.rs");
    assert.equal(imports.enrichment("role"), "");
    assert.equal(
      imports.validatorFn("datasource", "user", "validate_datasource_user"),
      "crate::types::generated::datasource::validators::validate_datasource_user",
    );
    assert.equal(
      imports.validatorFn("view", "user", "validate_user"),
      "crate::types::generated::views::validators::validate_user",
    );
    assert.equal(imports.apiPath("card_payment"), "card-payments");
    assert.equal(imports.apiPath("user"), "users");
    assert.equal(imports.frontend("src/main.rs"), "frontend/src/main.rs");
  });

  it("cases file names from settings for every lane", () => {
    assert.equal(layered().datasource("notification_type"), "notificationType.rs");
    assert.equal(layered().view("notification_type"), "notificationType.rs");
    assert.equal(
      layered().service("notification_type"),
      "notificationTypeService.rs",
    );
    const snake = createImportGenerator(".", {
      "languages.rust.casing.file_names": "Snake",
    });
    assert.equal(snake.datasource("notification_type"), "notification_type.rs");
    assert.equal(
      snake.service("notification_type"),
      "notification_type_service.rs",
    );
    const pascal = createImportGenerator(".", {
      "languages.rust.casing.file_names": "Pascal",
    });
    assert.equal(pascal.datasource("notification_type"), "NotificationType.rs");
    assert.equal(pascal.view("notification_type"), "NotificationType.rs");
  });

  it("cases type quals from settings independently of files", () => {
    const snakeTypes = createImportGenerator(".", {
      "languages.rust.casing.types": "Snake",
    });
    assert.equal(
      snakeTypes.datasourceQual("notification_type"),
      "crate::types::generated::datasource::notification_type",
    );
    assert.equal(
      snakeTypes.viewQual("user"),
      "crate::types::generated::views::user",
    );
  });

  it("treats organize_by_feature values other than true as layered", () => {
    for (const value of ["", "false", "TRUE", "1"]) {
      const imports = createImportGenerator(".", {
        "other.organize_by_feature": value,
      });
      assert.equal(imports.route("user"), "users.rs", value);
      assert.equal(imports.appWiring(), "app_wiring.rs", value);
    }
  });

  it("resolves custom stubs from module paths", () => {
    const imports = layered();
    assert.equal(imports.serviceCustom("UserService"), "../custom/userService.rs");
    assert.equal(imports.serviceCustom("UserService", undefined), "../custom/userService.rs");
    assert.equal(imports.serviceCustom("UserService", ""), "../custom/userService.rs");
    assert.equal(
      imports.serviceCustom("UserService", "./services/custom/user_service"),
      "../custom/user_service.rs",
    );
    assert.equal(
      imports.serviceCustom("UserService", "./custom/user_service"),
      "../custom/user_service.rs",
    );
    assert.equal(
      imports.serviceCustom("HealthCheckService", "./../custom/health_check_service"),
      "../custom/health_check_service.rs",
    );
    assert.equal(imports.routeCustom("GetHealthRoute"), "../custom/getHealthRoute.rs");
    assert.equal(
      imports.routeCustom("GetHealthRoute", "./routes/custom/get_health_route"),
      "../custom/get_health_route.rs",
    );
    assert.equal(imports.serviceCustom("Service"), "../custom/service.rs");
    assert.equal(imports.serviceCustom("---"), "../custom/---.rs");
    assert.equal(imports.serviceCustom("HealthCheck"), "../custom/healthCheck.rs");
    assert.equal(imports.serviceCustom("XMLParser"), "../custom/xmlParser.rs");
    assert.equal(imports.serviceCustom("user_service"), "../custom/userService.rs");
    assert.equal(
      imports.serviceCustom("UserService", "./../../custom/user_service"),
      "../custom/user_service.rs",
    );
  });
});

describe("RustImportGenerator by-feature", () => {
  it("nests files under features/ and qualifies types through the file module", () => {
    const imports = byFeature();
    assert.equal(imports.datasource("user"), "features/user/user.rs");
    assert.equal(imports.datasourceRel("user"), "features/user/user.rs");
    assert.equal(
      imports.datasourceQual("user"),
      "crate::features::user::user::User",
    );
    assert.equal(
      imports.datasourceValidator("user"),
      "features/user/userValidator.rs",
    );
    assert.equal(
      imports.datasourceValidatorRel("user"),
      "features/user/userValidator.rs",
    );
    assert.equal(
      imports.view("create_card_payment"),
      "features/cardPayment/createCardPayment.rs",
    );
    assert.equal(
      imports.view("update_card_payment"),
      "features/cardPayment/updateCardPayment.rs",
    );
    assert.equal(
      imports.viewValidator("create_card_payment"),
      "features/cardPayment/createCardPaymentValidator.rs",
    );
    assert.equal(imports.viewRel("user"), "features/user/user.rs");
    assert.equal(
      imports.viewQual("user"),
      "crate::features::user::user::User",
    );
    assert.equal(
      imports.viewValidatorRel("user"),
      "features/user/userValidator.rs",
    );
    assert.equal(imports.service("user"), "features/user/userService.rs");
    assert.equal(imports.serviceRel("user"), "features/user/userService.rs");
    assert.equal(
      imports.serviceTest("user"),
      "features/user/__tests__/userServiceTests.rs",
    );
    assert.equal(
      imports.serviceTestRel("user"),
      "features/user/__tests__/userServiceTests.rs",
    );
    assert.equal(
      imports.serviceIntegrationTest("user"),
      "features/user/__tests__/userServiceIntegrationTests.rs",
    );
    assert.equal(
      imports.serviceIntegrationTestRel("user"),
      "features/user/__tests__/userServiceIntegrationTests.rs",
    );
    assert.equal(
      imports.serviceCustomRel("user"),
      "features/user/custom/userService.rs",
    );
    assert.equal(
      imports.serviceUse("user", "UserService"),
      "use crate::features::user::userService::UserService;",
    );
    assert.equal(imports.route("user"), "features/user/usersRouter.rs");
    assert.equal(imports.routeRel("user"), "features/user/usersRouter.rs");
    assert.equal(imports.routeModule("user"), "usersRouter");
    assert.equal(
      imports.routeTest("user"),
      "features/user/__tests__/usersRouterTests.rs",
    );
    assert.equal(imports.index("features/user/user.rs"), "");
    assert.equal(
      imports.test("features/user/user.rs", "user"),
      "features/user/__tests__/userTests.rs",
    );
    assert.equal(imports.appWiring(), "features/app_wiring.rs");
    assert.equal(
      imports.validatorFn("datasource", "user", "validate_datasource_user"),
      "crate::features::user::userValidator::validate_datasource_user",
    );
    assert.equal(
      imports.validatorFn("view", "user", "validate_user"),
      "crate::features::user::userValidator::validate_user",
    );
    assert.equal(imports.frontend("src/lib.rs"), "frontend/src/lib.rs");
  });

  it("cases files and feature directories together for every lane", () => {
    const camel = byFeature();
    assert.equal(
      camel.datasource("notification_type"),
      "features/notificationType/notificationType.rs",
    );
    assert.equal(
      camel.view("notification_type"),
      "features/notificationType/notificationType.rs",
    );
    assert.equal(
      camel.service("notification_type"),
      "features/notificationType/notificationTypeService.rs",
    );
    const imports = byFeature({
      "languages.rust.casing.file_names": "Pascal",
      "languages.rust.casing.directories": "Kebab",
    });
    assert.equal(
      imports.datasource("notification_type"),
      "features/notification-type/NotificationType.rs",
    );
    assert.equal(
      imports.service("notification_type"),
      "features/notification-type/NotificationTypeService.rs",
    );
  });

  it("derives custom stub feature folders from class names", () => {
    const imports = byFeature();
    assert.equal(
      imports.serviceCustom("UserService"),
      "features/user/custom/userService.rs",
    );
    assert.equal(
      imports.serviceCustom("HealthCheckService"),
      "features/healthCheck/custom/healthCheckService.rs",
    );
    assert.equal(
      imports.routeCustom("GetHealthRoute"),
      "features/getHealth/custom/getHealthRoute.rs",
    );
    assert.equal(
      imports.serviceCustom("Service"),
      "features/service/custom/service.rs",
    );
    assert.equal(
      imports.serviceCustom("---"),
      "features/shared/custom/---.rs",
    );
    assert.equal(
      imports.serviceCustom("HealthCheck"),
      "features/healthCheck/custom/healthCheck.rs",
    );
    assert.equal(
      imports.serviceCustom("UserService", "features/user/custom/user"),
      "features/user/custom/userService.rs",
    );
    assert.equal(
      imports.serviceCustom("UserService", "./services/custom/user"),
      "features/user/custom/userService.rs",
    );
    assert.equal(
      imports.serviceCustom("UserService", "./routes/x"),
      "features/user/custom/userService.rs",
    );
  });

  it("accepts a module path under ./features/", () => {
    const imports = byFeature();
    assert.equal(
      imports.serviceCustom("UserService", "./features/user/custom/user_service"),
      "features/user/custom/user_service.rs",
    );
    assert.equal(
      imports.routeCustom("GetHealthRoute", "./features/health/custom/get_health_route"),
      "features/health/custom/get_health_route.rs",
    );
  });

  it("rejects a custom module outside ./features/", () => {
    const imports = byFeature();
    assert.throws(
      () => imports.serviceCustom("UserService", "./lib/user"),
      /generateCustomServiceStub: service "UserService" has module "\.\/lib\/user" which is outside \.\/features\//,
    );
    assert.throws(
      () => imports.routeCustom("GetHealthRoute", "./lib/route"),
      /generateCustomRouteStub: route "GetHealthRoute" has module "\.\/lib\/route" which is outside \.\/features\//,
    );
  });
});

describe("RustImportGenerator flat basePath", () => {
  it("ignores organize_by_feature and prefixes the directory", () => {
    const imports = flat("types/generated/datasource", {
      "other.organize_by_feature": "true",
    });
    assert.equal(imports.datasource("user"), "types/generated/datasource/user.rs");
    assert.equal(imports.datasourceRel("user"), "types/generated/datasource/user.rs");
    assert.equal(imports.index("types/generated/datasource/user.rs"), "types/generated/datasource/mod.rs");
  });

  it("treats empty basePath as backend layout, not flat", () => {
    const imports = createImportGenerator("", {
      "other.organize_by_feature": "true",
    });
    assert.equal(imports.datasource("user"), "features/user/user.rs");
    assert.equal(imports.appWiring(), "features/app_wiring.rs");
  });
});
