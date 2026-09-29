# Contributing

## Checks

CI runs these on every push and pull request to `master` (Windows only, since the app is):

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

## Commit messages

The changelog is generated from [Conventional Commits](https://www.conventionalcommits.org/)
by [git-cliff](https://git-cliff.org) (`cliff.toml`): `feat`, `fix`, `perf`, `refactor`,
`docs`, `style` and `test` are listed; `chore` and `ci` are not. Use `type(scope): summary`,
for example `fix(overlay): keep the toolbar on screen after a monitor change`.
Commits that are not conventional (including merge commits) are left out.

## Releasing

Releases are cut from `master` by the **Publish Release** workflow (Actions, Run workflow).
Give it an exact version (`0.2.0`) or `major` / `minor` / `patch`. It:

1. bumps `Cargo.toml` and `Cargo.lock`,
2. regenerates `CHANGELOG.md`,
3. commits `chore(release): prepare for vX.Y.Z`, tags `vX.Y.Z` and pushes both,
4. creates the GitHub release with notes for the new version,
5. publishes the crate to crates.io (on a Windows runner, since the crate only builds there).

The pushed tag then triggers **Build Release Binaries**, which builds `zoomify.exe`, zips it
with a SHA-256 checksum, and attaches both to the release.

### Repository secrets

| Secret | Used for |
|---|---|
| `RELEASE_GITHUB_TOKEN` | A personal access token with `contents: write` on this repo. The default `GITHUB_TOKEN` cannot be used: commits and tags it pushes do not trigger other workflows, so the binaries would never build. If `master` is protected, this token's owner must be allowed to push to it. |
| `CARGO_REGISTRY_TOKEN` | A [crates.io API token](https://crates.io/settings/tokens) scoped to publishing `zoomify`. |

If the crates.io step fails after the tag was pushed (for example a bad token), fix the
secret and re-run only that job; the tag and GitHub release already exist. If the failure
needs a code change, cut a new patch release instead of moving the tag.
