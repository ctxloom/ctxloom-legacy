# world: base
# NOT ported: nothing; Go read its library from disk on every call, and kept no record of it.

@doc
Feature: library changes — git holds the library, and the daemon records what changed in it, once, when a read sees it

  A library tree is a project checkout's `.ctxloom/` or the home's
  `~/.ctxloom/`. ctxloom's own content in it (`content/bundles/v2/**`,
  `config.yaml`, `remotes.yaml`, `lock.yaml`) is a git pathspec, and git
  holds it: ctxloom keeps no copy, no manifest and no digest of its own. A
  tree outside git is refused, and so is a tree git ignores. Nothing under
  `sessions/`, `state/` or `cache/` is the library.

  EVERY READ OBSERVES. The daemon answers every read of the library (a
  listing, a show, a search, context assemble, a dry run, a hook's read).
  First it asks git what it sees:
    - the commit checked out;
    - `git status` over the pathspec;
    - the object id of each file that differs.
  It compares that with what it last recorded for the tree. If anything
  differs, it records ONE observation, which lists each path that changed
  and how: added, modified, deleted, renamed or type-changed. Then it
  answers from the files as they are. A read that sees nothing new records
  nothing. Two reads that see the same change at once record it once.

  THE RECORD IS THIN. An observation names paths, how they changed, and the
  commits it saw. It never holds a file's text: git has that. The record
  lets ctxloom react to a change (a running session is told; see
  cli/daemon.feature), never restore one.

  EACH PATH HAS A SEQUENCE. Every event in a tree's record has a sequence,
  its place in that record, and an event that names a path ties the path's
  git id to that sequence. A path's last sequence is its tag (the etag a
  preview carries; a bundle's is the latest of its files'), so a write
  planned against a path is refused as stale once anything newer has been
  recorded of it, even a change undone since. An observation, the fact a
  read records, carries no sequence of its own: the record gives one to
  the event it makes.

  MOVES ARE GIT'S. A rename git reports, committed or staged, is a rename. A
  file moved without git is a delete and a create.

  ONE CHECKOUT, ONE RECORD. Each checkout is named once, by a harp kept in
  its `.ctxloom/state/checkout` (out of git), so a git worktree of the
  project is a checkout of its own. The home is `users/me/library`. A tree's
  first read records one baseline: the commit and whatever git sees
  uncommitted.

  THE HOME IS A REPOSITORY TOO. When the home's `.ctxloom` is in no git
  work tree, the daemon makes it one, once, unasked. Its `.gitignore` is an
  allowlist, so only ctxloom's own files are ever seen and nothing else in
  the home is tracked by accident. A home already inside a repository (a
  dotfiles one) is used as it is. ctxloom commits nothing in a project; the
  home's commits come with the writes (cli/atomic_writes.feature).

  The records are read here with the test build's
  `ctxloom test library-events [--home] --format json`, which prints the
  tree's stream:

    {"tree": "projects/<p>/checkouts/<c>/library",
     "events": [{"type": "TreeObserved", "sequence": n, "baseline": bool,
                 "head_before": "<oid>", "head_after": "<oid>",
                 "in_motion": bool,
                 "changes": [{"path", "status", "from_path", "oid"}]}]}

  A change's path is relative to the tree's root (`.ctxloom/`), with `/`
  separators. Changes are sorted by path. An event's `sequence` is its
  place in the tree's record; a change's `oid` is git's id for the path
  after it (absent when deleted).

  Rule: A library is in git, or it is refused

    Scenario Outline: A library read in a project outside git is refused, naming git init
      # new: G1, git is required.
      Given an initialized ctxloom project outside git
      When I run "<command>"
      Then the command exits with code 20
      And the refusal is "library-tree-not-in-git"
      And the output contains "git init"
      And the file ".ctxloom/state/checkout" does not exist

      Examples:
        | command                                  |
        | ctxloom bundle list                      |
        | ctxloom context assemble --format json   |
        | ctxloom run --dry-run                    |
        | ctxloom fragment list                    |

    Scenario Outline: A library git ignores is refused, naming the rule and its fix
      # new: G1, an ignored library is refused with doctor's fix.
      Given an initialized ctxloom project
      And the project already has the file ".gitignore":
        """
        <rule>
        """
      When I run "ctxloom bundle list"
      Then the command exits with code 20
      And the refusal is "library-tree-library-ignored"
      And the output contains ".gitignore"
      And the output contains "<rule>"
      And the output contains "ctxloom manage gitignore install"

      Examples:
        | rule       |
        | .ctxloom/  |
        | .ctxloom/* |

    Scenario: A project in a subdirectory of a repository is in git
      # new: in a work tree is enough; the repository's root need not be the project's.
      Given an initialized ctxloom project in a subdirectory of a git repository
      When I run "ctxloom bundle list"
      Then the command succeeds

    Scenario: With no git on the PATH, a read fails and says why
      # new: G11, git missing is exit 10.
      Given an initialized ctxloom project
      And git is not on the PATH
      When I run "ctxloom bundle list"
      Then the command exits with code 10
      And the error is "library-git-git-missing"
      And the output contains "on the PATH"

  Rule: The home is a git repository, made once, tracking ctxloom's own files alone

    Scenario: The first daemon makes the home's ctxloom directory a repository that sees only ctxloom's files
      # new: G2(a), the allowlist ignore.
      Given an initialized ctxloom project
      And the home already has the file ".ctxloom/records/2025-old.jsonl":
        """
        {"left": "by an older ctxloom"}
        """
      When I run "ctxloom bundle list"
      Then the command succeeds
      And the output contains "a git repository of ctxloom's own files"
      And the home's ".ctxloom" is a git repository with no commit
      And the home's file ".ctxloom/.gitignore" is exactly:
        """
        # ctxloom: only what is listed here is ctxloom's own, and seen by git.
        /*
        !/.gitignore
        !/config.yaml
        !/remotes.yaml
        !/lock.yaml
        !/content/
        """
      And git in the home's ".ctxloom" sees exactly these untracked files:
        | .gitignore  |
        | config.yaml |

    Scenario: A home made a repository is made once, and its .gitignore is hers from then on
      # new: G2, idempotent.
      Given an initialized ctxloom project
      And I run "ctxloom bundle list"
      And the home's file ".ctxloom/.gitignore" has the line "!/notes.md" added by hand
      And I run "ctxloom daemon stop"
      When I run "ctxloom bundle list"
      Then the command succeeds
      And the output does not contain "a git repository of ctxloom's own files"
      And the home's file ".ctxloom/.gitignore" contains "!/notes.md"

    Scenario: A home whose ctxloom directory is already a repository is used as it is
      # new: G2, an existing repository is never re-made.
      Given an initialized ctxloom project
      And the home's ".ctxloom" is already a git repository with the commit "mine"
      When I run "ctxloom bundle list"
      Then the command succeeds
      And the home's ".ctxloom" repository's last commit is still "mine"
      And the home's file ".ctxloom/.gitignore" does not exist

    Scenario: A home inside a dotfiles repository is used as it is, its ctxloom files alone observed
      # new: G2(a), a home already in a work tree.
      Given an initialized ctxloom project
      And the home directory is a git repository of its own
      When I run "ctxloom bundle list"
      Then the command succeeds
      And the home's ".ctxloom" is not a git repository of its own
      And the home's file ".gitignore" does not exist
      When I run "ctxloom test library-events --home --format json"
      Then the command succeeds
      And the output reports "tree" as "users/me/library"
      And the output reports "events[0].baseline" as "true"
      And the output reports "events[0].changes[0].path" as "config.yaml"
      And the output reports "events[0].changes[0].status" as "added"
      And the output does not contain "ctxloom.db"

  Rule: Each checkout is named once, and its first read records one baseline

    Scenario: The first read names the checkout and records its baseline; a read that sees nothing new records nothing
      # new: L2, L16; a read with no change records nothing.
      Given an initialized ctxloom project
      And a bundle "demo" exists
      And the project's changes are committed
      When I run "ctxloom bundle list"
      And I run "ctxloom bundle list"
      And I run "ctxloom fragment show demo#fragments/example"
      And I run "ctxloom test library-events --format json"
      Then the command succeeds
      And the file ".ctxloom/state/checkout" exists
      And the output reports "tree" containing "/checkouts/"
      And the output reports "events" having "1" entries
      And the output reports "events[0].type" as "TreeObserved"
      And the output reports "events[0].baseline" as "true"
      And the output reports "events[0].head_after" as the project's commit "HEAD"
      And the output reports "events[0].changes" having "0" entries

    Scenario: A baseline lists what git sees uncommitted, and nothing of what it holds
      # new: L16, the baseline is one small fact.
      Given an initialized ctxloom project
      And a bundle "demo" exists
      When I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the command succeeds
      And the output reports "events[0].baseline" as "true"
      And the output reports "events[0].changes" having "3" entries
      And the output reports "events[0].changes[0].path" as "content/bundles/v2/demo/bundle.yaml"
      And the output reports "events[0].changes[0].status" as "added"
      And the output does not contain "Add your content here"

    Scenario: A project with no commit yet is baselined on its untracked files
      # new: a fresh `git init` has no HEAD.
      Given an empty project directory
      And I run "ctxloom init --format json"
      When I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the command succeeds
      And the output reports "events[0].baseline" as "true"
      And the output reports "events[0].head_after" as ""
      And the output reports "events[0].changes[0].path" as "config.yaml"
      And the output reports "events[0].changes[0].status" as "added"

    Scenario: A git worktree of the project is a checkout of its own, with its own record
      # new: L2, a worktree resolves to its primary's project but is its own tree.
      Given an initialized ctxloom project
      And the project's changes are committed
      And I run "ctxloom bundle list"
      And I run git 'worktree add -q ../wt' in the project
      When I run "ctxloom bundle list" in the directory "../wt"
      And I run "ctxloom test library-events --format json" in the directory "../wt"
      Then the command succeeds
      And the output reports "events" having "1" entries
      And the output reports "events[0].baseline" as "true"
      And the file "../wt/.ctxloom/state/checkout" is not the same as ".ctxloom/state/checkout"
      When I run "ctxloom test library-events --format json"
      Then the output reports "events" having "1" entries

  Rule: A change made outside ctxloom is recorded once, by the next read that sees it

    Background:
      Given an initialized ctxloom project
      And a bundle "demo" exists
      And the project's changes are committed
      And I run "ctxloom bundle list"

    Scenario: A hand edit is served at once, and recorded once
      # new: L5, L6; an outside change is a fact.
      When the file ".ctxloom/content/bundles/v2/demo/fragments/example.md" is changed by hand to:
        """
        # Edited by hand
        """
      And I run "ctxloom fragment show demo#fragments/example"
      Then the command succeeds
      And the output contains "Edited by hand"
      When I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "2" entries
      And the output reports "events[1].baseline" as "false"
      And the output reports "events[1].in_motion" as "false"
      And the output reports "events[1].changes" having "1" entries
      And the output reports "events[1].changes[0].path" as "content/bundles/v2/demo/fragments/example.md"
      And the output reports "events[1].changes[0].status" as "modified"
      And the output does not contain "Edited by hand"

    Scenario: A second edit of a file git already sees modified is recorded too
      # new: L8, the dirty paths' object ids, since `git status` says " M" for both edits;
      # each edit is an event of its own, so the path's sequence moves twice (G4 superseded, 2026-11-16).
      When the file ".ctxloom/content/bundles/v2/demo/fragments/example.md" is changed by hand to:
        """
        first edit
        """
      And I run "ctxloom bundle list"
      And the file ".ctxloom/content/bundles/v2/demo/fragments/example.md" is changed by hand to:
        """
        second edit
        """
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "3" entries
      And the output reports "events[2].changes[0].path" as "content/bundles/v2/demo/fragments/example.md"
      And the output reports "events[2].changes[0].status" as "modified"

    Scenario: A file edited, put back and edited again is three changes, each recorded
      # new: L4, the chain over the last cause; A to B to A to B never collides.
      When the file ".ctxloom/content/bundles/v2/demo/bundle.yaml" is changed by hand to:
        """
        schema_version: 4
        version: 2.0.0
        """
      And I run "ctxloom bundle list"
      And I run git 'checkout -q -- .ctxloom/content/bundles/v2/demo/bundle.yaml' in the project
      And I run "ctxloom bundle list"
      And the file ".ctxloom/content/bundles/v2/demo/bundle.yaml" is changed by hand to:
        """
        schema_version: 4
        version: 2.0.0
        """
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "4" entries
      And the output reports "events[2].changes[0].path" as "content/bundles/v2/demo/bundle.yaml"
      And the output reports "events[3].changes[0].path" as "content/bundles/v2/demo/bundle.yaml"

    Scenario: A commit made outside ctxloom is recorded once, between the commits it moved
      # new: diff-tree between the commits last seen and now.
      When the file ".ctxloom/content/bundles/v2/demo/fragments/example.md" is changed by hand to:
        """
        committed by hand
        """
      And I run git 'commit -qam "edit the fragment"' in the project
      And I run "ctxloom bundle list"
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "2" entries
      And the output reports "events[1].head_before" as the project's commit "HEAD~1"
      And the output reports "events[1].head_after" as the project's commit "HEAD"
      And the output reports "events[1].changes[0].path" as "content/bundles/v2/demo/fragments/example.md"
      And the output reports "events[1].changes[0].status" as "modified"

    Scenario: A commit that touches nothing of the library records nothing
      # new: the library's tree is unchanged, so nothing differs.
      When the file "README.md" is changed by hand to:
        """
        not the library
        """
      And I run git 'add README.md' in the project
      And I run git 'commit -qm "a readme"' in the project
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "1" entries

    Scenario: A branch switch is one observation listing every path it changed
      # new: Q6, a branch switch.
      When I run git 'checkout -qb other' in the project
      And the file ".ctxloom/content/bundles/v2/demo/fragments/example.md" is changed by hand to:
        """
        on the other branch
        """
      And the file ".ctxloom/content/bundles/v2/demo/prompts/example.md" is changed by hand to:
        """
        on the other branch too
        """
      And I run git 'commit -qam "other"' in the project
      And I run "ctxloom bundle list"
      And I run git 'checkout -q -' in the project
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "3" entries
      And the output reports "events[2].head_before" as the project's commit "other"
      And the output reports "events[2].head_after" as the project's commit "HEAD"
      And the output reports "events[2].changes" having "2" entries
      And the output reports "events[2].changes[0].path" as "content/bundles/v2/demo/fragments/example.md"
      And the output reports "events[2].changes[0].status" as "modified"
      And the output reports "events[2].changes[1].path" as "content/bundles/v2/demo/prompts/example.md"

    Scenario: Switching to a branch and back before any read records nothing
      # new: what is compared is what was last recorded, not what happened between.
      When I run git 'checkout -qb other' in the project
      And I run git 'checkout -q -' in the project
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "1" entries

    Scenario: Two reads that see one change at once record it once
      # new: L4, the fact's key; the second is a duplicate.
      When the file ".ctxloom/content/bundles/v2/demo/fragments/example.md" is changed by hand to:
        """
        seen twice at once
        """
      And "ctxloom bundle list" is run twice at once
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "2" entries

    Scenario Outline: A change outside the library's paths records nothing
      # new: L13, the pathspec.
      When the file "<path>" is changed by hand to:
        """
        not the library
        """
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "1" entries

      Examples:
        | path                                            |
        | .ctxloom/sessions/brisk-copper-moth/notes.md    |
        | .ctxloom/state/scratch                          |
        | .ctxloom/cache/repos/example.com/acme/HEAD      |
        | src/main.rs                                     |

    Scenario Outline: The config, the remotes and the lock are the library's, and a read records them even when it cannot use them
      # new: L13, the pathspec; the observation comes before the read parses anything.
      When the file ".ctxloom/<file>" is changed by hand to:
        """
        # changed by hand
        """
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events" having "2" entries
      And the output reports "events[1].changes[0].path" as "<file>"
      And the output reports "events[1].changes[0].status" as "<status>"

      Examples:
        | file         | status   |
        | config.yaml  | modified |
        | remotes.yaml | added    |
        | lock.yaml    | added    |

  Rule: Moves are git's: a rename where git reports one, else a delete and a create

    Background:
      Given an initialized ctxloom project
      And a bundle "demo" exists
      And the project's changes are committed
      And I run "ctxloom bundle list"

    Scenario: A committed git mv is recorded as a rename
      # new: L7, diff-tree -M.
      When I run git 'mv .ctxloom/content/bundles/v2/demo/fragments/example.md .ctxloom/content/bundles/v2/demo/fragments/renamed.md' in the project
      And I run git 'commit -qm "rename"' in the project
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events[1].changes" having "1" entries
      And the output reports "events[1].changes[0].status" as "renamed"
      And the output reports "events[1].changes[0].path" as "content/bundles/v2/demo/fragments/renamed.md"
      And the output reports "events[1].changes[0].from_path" as "content/bundles/v2/demo/fragments/example.md"

    Scenario: A git mv not yet committed is recorded as a rename, as git status pairs it
      # new: L7, porcelain v2's rename entry.
      When I run git 'mv .ctxloom/content/bundles/v2/demo/fragments/example.md .ctxloom/content/bundles/v2/demo/fragments/renamed.md' in the project
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events[1].changes" having "1" entries
      And the output reports "events[1].changes[0].status" as "renamed"
      And the output reports "events[1].changes[0].from_path" as "content/bundles/v2/demo/fragments/example.md"

    Scenario: A file moved without git is recorded as a delete and a create
      # new: L7, nothing pairs an untracked file with a deleted one.
      When the file ".ctxloom/content/bundles/v2/demo/fragments/example.md" is moved by hand to ".ctxloom/content/bundles/v2/demo/fragments/moved.md"
      And I run "ctxloom bundle list"
      And I run "ctxloom test library-events --format json"
      Then the output reports "events[1].changes" having "2" entries
      And the output reports "events[1].changes[0].path" as "content/bundles/v2/demo/fragments/example.md"
      And the output reports "events[1].changes[0].status" as "deleted"
      And the output reports "events[1].changes[1].path" as "content/bundles/v2/demo/fragments/moved.md"
      And the output reports "events[1].changes[1].status" as "added"

  Rule: While git is in the middle of an operation, a read records what it sees and says so

    Scenario Outline: A read during a git operation is answered, recorded as in motion, and says so
      # new: L11; a write then is refused (cli/atomic_writes.feature, 10b).
      Given an initialized ctxloom project
      And a bundle "demo" exists
      And the project's changes are committed
      And I run "ctxloom bundle list"
      And git in the project is in the middle of <operation> on ".ctxloom/content/bundles/v2/demo/fragments/example.md"
      When I run "ctxloom bundle list --format json"
      Then the command succeeds
      And the output contains "<said>"
      When I run "ctxloom test library-events --format json"
      Then the output reports "events" having "2" entries
      And the output reports "events[1].in_motion" as "true"
      And the output reports "events[1].changes[0].path" as "content/bundles/v2/demo/fragments/example.md"

      Examples:
        | operation                           | said              |
        | a merge stopped on a conflict       | a merge           |
        | a rebase stopped on a conflict      | a rebase          |
        | a commit that holds the index lock  | holds the index   |
