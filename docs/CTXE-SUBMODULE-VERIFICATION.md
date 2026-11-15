# ctxe submodule verification

Issue: [#19](https://github.com/krlybnd/krlyxd/issues/19)
Verified: 2026-11-14
Superproject base: `1aac484a594f1e1f7176fd0309d9d5e04f88ae0b`

## Current repository metadata

These are observations from the superproject at the base commit; they do not prescribe a future update policy.

| Item | Recorded value |
| --- | --- |
| Superproject remote | `https://github.com/krlybnd/krlyxd.git` |
| Superproject branch containing the base | `rebuild/core` |
| Superproject default remote branch | `origin/rebuild/core` |
| Submodule path | `.ctx/ctxe` |
| Submodule URL (`.gitmodules`) | `https://github.com/krlybnd/ctxe.git` |
| Gitlink commit | `cac12f38e5cef691199a81302810cdcd1cd31146` |
| `.gitmodules` branch field | None |
| Remote branch at the gitlink commit | `feat/cursor-plugin` |
| Remote `main` tip observed | `70342ff4f3c2f02db94cb8f2ff60f4225dc6ca6b` |

The superproject tree records `.ctx/ctxe` as mode `160000` at the gitlink SHA above. The submodule has its own remote `origin`, at the URL in `.gitmodules`. Its `feat/cursor-plugin` ref resolved to the gitlink SHA during verification; `main` was at a different commit. The issue description's branch and short SHA therefore match the current remote metadata.

## Clean-clone verification

Commands run from a separate temporary directory, not from the existing submodule checkout:

```sh
git clone --recurse-submodules https://github.com/krlybnd/krlyxd.git /tmp/krlyxd-issue19.zVUBiC/clone
git -C /tmp/krlyxd-issue19.zVUBiC/clone status --short --branch
git -C /tmp/krlyxd-issue19.zVUBiC/clone rev-parse HEAD
git -C /tmp/krlyxd-issue19.zVUBiC/clone submodule status --recursive
git -C /tmp/krlyxd-issue19.zVUBiC/clone checkout --detach 1aac484a594f1e1f7176fd0309d9d5e04f88ae0b
git -C /tmp/krlyxd-issue19.zVUBiC/clone submodule update --init --recursive
git -C /tmp/krlyxd-issue19.zVUBiC/clone status --short --branch
git -C /tmp/krlyxd-issue19.zVUBiC/clone rev-parse HEAD
git -C /tmp/krlyxd-issue19.zVUBiC/clone submodule status --recursive
git -C /tmp/krlyxd-issue19.zVUBiC/clone/.ctx/ctxe remote -v
git -C /tmp/krlyxd-issue19.zVUBiC/clone/.ctx/ctxe branch -a -vv
git -C /tmp/krlyxd-issue19.zVUBiC/clone/.ctx/ctxe show -s --format=fuller HEAD
git ls-remote --heads --tags https://github.com/krlybnd/ctxe.git
```

Result:

- `git clone --recurse-submodules` completed successfully. The clone selected `rebuild/core` at `1aac484a594f1e1f7176fd0309d9d5e04f88ae0b` and initialized `.ctx/ctxe` at `cac12f38e5cef691199a81302810cdcd1cd31146`.
- The explicit detached checkout of the requested base succeeded. The subsequent recursive update succeeded and left the same gitlink checked out.
- Final superproject status was `## HEAD (no branch)` with no changed or untracked paths. `git submodule status --recursive` reported `cac12f38e5cef691199a81302810cdcd1cd31146 .ctx/ctxe (remotes/origin/feat/cursor-plugin)` with no `-`, `+`, or `U` prefix.
- The submodule checkout was detached at `cac12f38e5cef691199a81302810cdcd1cd31146`; its fetched `origin/feat/cursor-plugin` points to that commit, and `origin/main` points to `70342ff4f3c2f02db94cb8f2ff60f4225dc6ca6b`.
- The clone's submodule remote URL was `https://github.com/krlybnd/ctxe.git`. The remote ref listing confirmed both branch tips above.

## Adopted reproducible pin policy

Keep the superproject gitlink as the authoritative pin. A Git submodule checkout is reproducible because the superproject records an exact commit; a branch name is mutable metadata and does not itself select the checkout in an ordinary clone. The successful recursive clone demonstrates that the currently pinned commit is fetchable from the configured URL using this environment’s Git access.

For routine updates, choose and review a specific `ctxe` commit, update the gitlink deliberately, and commit that gitlink change in the superproject. Treat the source branch (currently `feat/cursor-plugin`) as provenance for the selected commit, not as a moving dependency. Do not add a `.gitmodules` branch field unless maintainers intentionally adopt `git submodule update --remote` as an update workflow; that field does not replace the gitlink pin and can make updates depend on remote branch movement. If maintainers later decide the dependency should follow `main`, select and review a commit from `main` and update the gitlink in a separate, explicit change.

Adopted on 2026-11-14 during the issue-closure audit: retain the existing exact gitlink
pin, with no automatic branch tracking. No submodule update is required. Verification
and this decision did not change `.gitmodules`, the gitlink, or any submodule working tree.
