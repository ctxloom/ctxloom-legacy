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

# Step 10b (texts.md §3, the 10b red spec's): said by a removal applied
# into the trash (bundle remove, skill remove, clean); reviewed before
# release with the rest, `connected-boil`.
library-api-notice-trashed = { $path } is kept in { $slot } for 7 days, in case you want it back; after that ctxloom removes it

# Step 10b (G3, owner 2026-11-18, night): DRAFT, for the owner's review. Said
# when a write to a home repository ctxloom made stands but its commit failed.
library-api-notice-home-not-committed = the change was written to { $path }, but ctxloom could not commit it there ({ $ext_err_txt }); it stays uncommitted until you commit it
