# Messages for the crate's errors. Each id is an error's code, which begins with
# the crate's name; the variables are its args. `library-tree-plan-rejected`
# has no message: a plan's rejection is told in its own diagnostic's words
# (L14).

# Step 10a (texts.md §1): accepted verbatim (owner, 2026-11-16, night;
# reviewed before release, `connected-boil`).
library-tree-not-in-git = { $root } is not in a git repository. ctxloom keeps a project's library in git, and reads none outside it.
library-tree-not-in-git-fix = run `git init` there (ctxloom init does it for you), then run the command again
library-tree-library-ignored = git ignores this project's library: line { $line } of { $source } says `{ $pattern }`, so nothing in .ctxloom is in git, and ctxloom reads none of it.
library-tree-library-ignored-fix = take that line out, and keep ctxloom's own state out of git with `ctxloom manage gitignore install`

# Step 10b (its texts.md §1): accepted verbatim with 10b's red spec (owner,
# 2026-11-16, night); raised once 10b sends the commands.
library-tree-plan-stale = { $path } changed after this change was planned (it was at change { $seen } of this library's record, it is now at change { $now }), so nothing was written
library-tree-plan-stale-fix = look again at what is there now, then run the command again
library-tree-resource-exists = { $path } already exists, so nothing was created over it
library-tree-resource-exists-fix = choose another name, or remove the one there first
library-tree-no-such-resource = there is nothing at { $path } to change
library-tree-builtin-read-only = { $bundle } is ctxloom's built-in bundle, and it is read-only
library-tree-builtin-read-only-fix = to change it, override it with a bundle of your own: `ctxloom bundle export { $bundle } <bundles dir>`
library-tree-tree-in-motion = git is in the middle of { $motion ->
    [index-locked] a commit or another change: another git process holds the index (.git/index.lock)
    [merging] a merge
    [rebasing] a rebase
    [cherry-picking] a cherry-pick
   *[reverting] a revert
  } in { $root }, so ctxloom changes nothing there until it ends
library-tree-tree-in-motion-fix = finish or abort it (`git merge --continue`, `git rebase --abort` …), then run the command again
