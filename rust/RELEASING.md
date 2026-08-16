# Releasing the `deterministic` cargo crate

This crate is published to the private **`deterministic-code`** alternative registry
(sparse index `https://deterministic-code.com/registry/`), which is served by the
backend's own registry service (`backend/src/services/custom/registry/`). The
`crate-v*` tag namespace is separate from the monorepo's other releases so the
crate can ship on its own patch cadence.

Consumers in `library_reference_mode: registry` pin the exact version
(`deterministic = { version = "= <x.y.z>", registry = "deterministic-code" }`,
emitted by `scripts/lib/emit-backend-app-rust.mjs`), so the published registry
version **must** match `rust/Cargo.toml`.

## Release steps

1. **Bump the version** in `rust/Cargo.toml`, open a PR, merge to `main`.

2. **Publish by tagging** — the canonical trigger:

   ```bash
   git fetch origin main
   git tag crate-v0.1.0 origin/main      # version MUST equal rust/Cargo.toml
   git push origin crate-v0.1.0
   ```

   This fires `.github/workflows/publish-deterministic-crate.yml`, which runs
   `scripts/publish-deterministic-crate.mjs` → `cargo publish --registry
   deterministic-code`. The script refuses if the tag version ≠
   `rust/Cargo.toml#version`.

3. **Verify it actually landed** — check the index, not just the green check:

   ```bash
   curl -s https://deterministic-code.com/registry/de/te/deterministic \
     | grep -o '"vers":"[^"]*"'
   ```

   The new version should appear.

### Dry run (optional)

Validate packaging without uploading (does not consume the version):

```bash
gh workflow run publish-deterministic-crate.yml -f version=0.1.0 -f dry_run=true
```

`workflow_dispatch` with `dry_run=false` also performs a real publish, if you
prefer that over pushing a tag.

## Invariants — keep these true or the publish 401s / CI goes red

- **The GitHub secret must equal the server's token.**
  `DETERMINISTIC_REGISTRY_PUBLISH_TOKEN` exists in two places that must match:
  the GitHub Actions repo secret (cargo sends it as the bearer) and the droplet's
  container env (`scripts/deploy-droplet.mjs` forwards it from the laptop `.env`
  at deploy time). Rotating it means changing **both**; a mismatch is
  `401 Unauthorized: invalid publish token`.

- **Publish in lockstep with the version bump.** `samples/email-backend` uses
  `registry` mode, so the `crate-registry-drift` guard
  (`scripts/check-crate-registry-drift.mjs`, `.github/workflows/crate-registry-drift.yml`)
  fails whenever `rust/Cargo.toml` is ahead of the published registry. A red drift
  guard means a version was bumped but not published — tag it.

- **Never `git push --no-verify` the tag.** The pre-push hook runs the full
  unit+integration gate even for a tag push. If it flakes, re-run the push; do
  not bypass.

## First-time / broken-pipeline setup

If publishes fail before reaching cargo, the publish workflow or its
prerequisites are broken, not the crate:

- The publish workflow runs `node scripts/publish-deterministic-crate.mjs` with
  no `npm install`, so that script must stay dependency-free (node builtins +
  its local `spawn-async.mjs` only) — don't add bare `@deterministic-code/*`
  imports to it.
- The registry server must have `DETERMINISTIC_REGISTRY_PUBLISH_TOKEN` set (else
  it returns `registry publish is not configured on this deployment`). It's set
  via the droplet container env — see `deploy-droplet.mjs`.
