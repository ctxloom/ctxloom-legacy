# Messages for the crate's notices. Each id is a notice's code, which begins
# with the crate's name; the variables are its args.

# Step 10a (texts.md §3, §4): accepted verbatim (owner, 2026-11-16, night;
# reviewed before release, `connected-boil`).
library-api-notice-tree-in-motion = git is in the middle of { $motion ->
    [index-locked] a commit or another change: another git process holds the index (.git/index.lock)
    [merging] a merge
    [rebasing] a rebase
    [cherry-picking] a cherry-pick
   *[reverting] a revert
  } in { $root }; this read shows the files as they are now, and ctxloom will make no change there until it ends
library-api-notice-home-repository-made = ctxloom made { $path } a git repository of ctxloom's own files (its .gitignore lists them), so it can see what changes in your home library. It commits there only when it writes.
