# Rust continuous integration

The [Rust workflow](../.github/workflows/rust.yml) validates PRs targeting `main`,
pushes to `main` and manual requests. It does not deploy, publish or merge.
Opening a draft PR enables checks on subsequent pushes without running duplicate
branch-push and PR pipelines. Superseded runs for the same PR/ref are canceled.

## Checks and inputs

- `Formatting`: rustfmt on Ubuntu 24.04.
- `Check (ubuntu-24.04)`: core workspace tests/doctests and all-target Clippy.
- `Check (windows-2025)`: the same checks on Windows Server 2025 x64.
- `Check (macos-26)`: the same checks plus macOS native test functions on ARM64.
- `Rust CI`: succeeds only when formatting and the entire platform matrix succeed;
  failures, cancellation and skipped prerequisites cannot produce a passing gate.

`rust-toolchain.toml` supplies the exact Rust version/components; no floating stable
toolchain or reduced KDF profile is substituted. Each check fetches `Cargo.lock`
dependencies with `--locked`, then runs tests/Clippy with `--locked --offline`.
Cargo tests compile the library/tests/examples and include doctests; a duplicate
standalone build is not needed for this library-only workspace.

Default PR checkout validates GitHub's test merge revision. Logs record that tested
SHA and toolchain, which must be associated with the PR's current head/base. A green
old commit or a canceled/skipped run is not evidence for later changes. Runner
labels bound OS families, not immutable images or the full desktop support matrix.
Windows Server is not Windows 11 desktop validation, and hosted macOS is not proof
of the exact local storage/OS build, native commit durability or physical power loss.

## Workflow dependency review

Reviewed 2026-11-02 using official release notes and action descriptors:

| Action | Version | Pinned commit | Purpose |
| --- | --- | --- | --- |
| [actions/checkout](https://github.com/actions/checkout/releases/tag/v7.0.1) | 7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` | Event revision checkout with persisted credentials disabled |
| [actions/cache](https://github.com/actions/cache/releases/tag/v6.1.0) | 6.1.0 | `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` | OS/architecture/toolchain/lockfile-scoped registry cache |

Both actions use Node 24 on standard GitHub-hosted runners. Only registry index
and downloaded archives are cached; builds, Cargo config, credentials and private
inputs are excluded. Cache misses still fetch the locked graph and perform all
checks. Action updates require reviewing the release and changing the complete SHA.
This bounded integration review is not an audit of bundled dependencies.

The workflow uses `pull_request`, never privileged `pull_request_target`, with
`contents: read` and no configured secrets. It does not persist Git authentication
or upload vaults/test directories. Tests use synthetic data. Hosted execution is
development infrastructure, not a hosted runtime requirement for Codeward.

## Adoption and required checks

Bootstrap: verify real PR runs on all three platforms and locally exercise the gate
across success/failure/canceled/skipped inputs before requiring `Rust CI` in the main ruleset. Keep existing protections
and require the GitHub Actions check source. A manual-only run does not substitute
for a PR check. This document describes the configuration; observed runs and rule
activation are recorded after verification, without implying merge approval.

Official references: [Rust workflow guide](https://docs.github.com/en/actions/tutorials/build-and-test-code/rust),
[runner matrix](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
[rustup toolchain files](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file),
[required-check behavior](https://docs.github.com/en/pull-requests/reference/status-checks).
