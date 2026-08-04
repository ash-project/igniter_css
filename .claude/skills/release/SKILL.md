---
name: release
description: Cut an igniter_css release. Use when asked to release, tag, publish to Hex, bump the version, or when a release failed. Covers the tag-triggered CI flow, the precompiled NIF matrix, and the checksum file.
---

# Release igniter_css

A tag is the only trigger. Everything after it is automatic — do not run the
checksum or publish steps by hand unless CI is broken.

## Steps

1. Bump `@version` in `mix.exs`.
2. Add a `# Changelog for IgniterCss X.Y.Z` section to `CHANGELOG.md` — the
   GitHub release notes are extracted from it by heading match.
3. Commit and push to `main`.
4. `git tag vX.Y.Z && git push origin vX.Y.Z`

## What CI then does

| job | result |
|---|---|
| checks | credo, dialyzer, test, format, sobelow, reuse, cargo test/fmt/clippy |
| `build-release` | 10 targets → `libigniter_css-vX.Y.Z-nif-2.15-<target>.so.tar.gz` attached to the GitHub release |
| `hex_publish` | `mix rustler_precompiled.download IgniterCss.Native --only-local --all --print` → `checksum-Elixir.IgniterCss.Native.exs`, then `mix hex.publish` |
| `github_release` | release notes from CHANGELOG |

## Invariants

- `checksum-Elixir.IgniterCss.Native.exs` is **never committed**. It hashes
  artifacts that do not exist until the tag builds. It is listed in `files:` in
  `mix.exs` so it ships in the Hex package.
- `targets:` in `lib/igniter_css/native.ex` must match the CI matrix exactly. A
  target built but not listed is never downloaded; one listed but not built is a
  hard failure for those users.
- `release:` is not passed in `.github/workflows/elixir.yml`, so it defaults to
  `true`. That is what enables `hex_publish`.

## Failure modes

| symptom | cause |
|---|---|
| `startup_failure`, "workflow file issue" | caller lacks `permissions:` the callee needs. `elixir.yml` must grant `contents/pages/id-token/security-events: write` |
| `Could not mix rebar from any hex.pm mirror` | OTP too old for hex.pm's cert chain. Needs OTP 28+ |
| `Hex.State ... does not exist` | poisoned `mix-home`/`hex-home` Actions cache from a failed run on a different OTP. Delete the caches via `gh api -X DELETE repos/OWNER/REPO/actions/caches/ID` and re-run |
| 404 downloading the NIF locally | no release exists yet. Use `IGNITERCSS_BUILD=1 mix compile` |

## Manual fallback

```bash
mix rustler_precompiled.download IgniterCss.Native --all --print
mix hex.build --unpack
mix hex.publish
```
