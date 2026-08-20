import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createCasing, DEFAULT_CASING } from "./default-casing.ts";

const NAME = "notification_type";

describe("createCasing Auto defaults", () => {
  it("matches Default Casings for Rust", () => {
    assert.deepEqual(DEFAULT_CASING, {
      file_names: "Camel",
      types: "Pascal",
      fields: "Snake",
      directories: "Camel",
    });
    const casing = createCasing({});
    assert.equal(casing.convertFileName(NAME), "notificationType");
    assert.equal(casing.convertTypes(NAME), "NotificationType");
    assert.equal(casing.convertFields(NAME), "notification_type");
    assert.equal(casing.convertDirectories(NAME), "notificationType");
    assert.equal(casing.filePath(NAME), "notificationType.rs");
    assert.equal(casing.serviceClassName("user"), "UserService");
  });

  it("puts Auto files under a cased feature directory", () => {
    const casing = createCasing({ "other.organize_by_feature": "true" });
    assert.equal(
      casing.filePath(NAME),
      "features/notificationType/notificationType.rs",
    );
  });
});

describe("createCasing overrides", () => {
  it("snakes file names", () => {
    const casing = createCasing({
      "languages.rust.casing.file_names": "Snake",
    });
    assert.equal(casing.filePath(NAME), "notification_type.rs");
  });

  it("pascals file names", () => {
    const casing = createCasing({
      "languages.rust.casing.file_names": "Pascal",
    });
    assert.equal(casing.filePath(NAME), "NotificationType.rs");
  });

  it("kebabs directories with snake files", () => {
    const casing = createCasing({
      "other.organize_by_feature": "true",
      "languages.rust.casing.file_names": "Snake",
      "languages.rust.casing.directories": "Kebab",
    });
    assert.equal(
      casing.filePath(NAME),
      "features/notification-type/notification_type.rs",
    );
  });

  it("camels fields", () => {
    const casing = createCasing({
      "languages.rust.casing.fields": "Camel",
    });
    assert.equal(casing.convertFields("role_id"), "roleId");
  });

  it("pascals fields", () => {
    const casing = createCasing({
      "languages.rust.casing.fields": "Pascal",
    });
    assert.equal(casing.convertFields("role_id"), "RoleId");
  });
});
