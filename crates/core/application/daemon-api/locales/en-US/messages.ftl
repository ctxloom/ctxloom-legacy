daemon-api-refused = the daemon refused with { $ext_err_txt }
daemon-api-failed = the daemon could not answer: { $ext_err_txt }
daemon-api-name-invalid = { $name } is not a resource name: { $reason }
daemon-api-session-not-found = no session is named { $name }
daemon-api-project-not-found = no project is named { $name }
daemon-api-run-not-found = the daemon hosts no run named { $name }
daemon-api-unimplemented = the daemon does not serve { $method } yet
daemon-api-transcript-empty = { $name } has no captured transcript yet
daemon-api-project-unknown = { $cwd } is in no ctxloom project
daemon-api-daemon-hosting = the daemon hosts { $runs } live run(s): { $harps }
daemon-api-daemon-hosting-fix = wait for them to end, or end them with it: ctxloom daemon stop --even-if-live
# OD-62 (2026-11-15): DRAFT wording, the owner's to approve (the successor
# version is gone).
daemon-api-draining = the serving daemon, ctxloom { $daemon }, is shutting down and takes no new call; it still hosts: { $harps }
daemon-api-draining-fix = run the command again once it has exited (the next daemon resumes each child it handed over); to stop what it hosts now: ctxloom shutdown now
daemon-api-age-invalid = { $key } { $value } is neither an age nor a date
daemon-api-age-invalid-fix = give an age such as 90d, 1h30m or 1.5d, or a date such as 2026-01-01
daemon-api-config-unreadable = the configuration of the project at { $root } does not read: { $ext_err_txt }
# OD-62 (2026-11-15): one ctxloom version runs at a time. DRAFT wording,
# the owner's to approve.
daemon-api-version-skew = the serving daemon is ctxloom { $daemon }, not this ctxloom { $own } (it hosts: { $harps })
daemon-api-version-skew-fix = stop it with `ctxloom daemon stop` (`--even-if-live` ends what it hosts), then run the command again
daemon-api-assembly-missing-fragment = no fragment is named { $name }
daemon-api-assembly-missing-tag = no fragment is tagged { $name }
daemon-api-launch-in-progress = another launch of { $harp } is being made now
daemon-api-launch-in-progress-fix = wait for it to finish, then look at the child: ctxloom session show { $harp }
daemon-api-launch-lock = a child's launch lock could not be taken: { $ext_err_txt }
# OD-52: DRAFT wording, the owner's to approve.
daemon-api-start-up-incomplete = the daemon's start-up catch-up failed: { $why }; what it shows (sessions, transcripts, what it hosts) may be missing events until a later catch-up succeeds
daemon-api-start-up-incomplete-fix = the daemon tries the catch-up again each round, and this warning clears once one succeeds; the daemon's log (daemon/daemon.log under ctxloom's home) says why it failed
daemon-api-handing-over = the daemon, ctxloom { $daemon }, is shutting down: { $harps } will be handed over to the next daemon; a child busy in a turn has been steered to end it, and is handed over within { $seconds } seconds at the latest, its turn in flight cut; the next daemon resumes each in its own conversation
# Step 8 U4 (texts.md §5, D14): accepted verbatim (owner, 2026-11-16; reviewed before release).
daemon-api-session-not-hosted = { $name } is no session this daemon runs, so its ask was not recorded
daemon-api-approval-ambiguous = { $harp } has { $count } asks waiting ({ $asks }), so its name does not say which you mean; nothing was answered.
daemon-api-approval-ambiguous-fix = Name the ask itself: `ctxloom session approvals answer <ask> <option>`, with one of { $asks }.
daemon-api-approval-none-waiting = { $harp } has no ask waiting, so nothing was answered.
daemon-api-approval-none-waiting-fix = List what waits with `ctxloom session approvals`, and name the ask (`ask-...`) to answer one that is settled or another session's.
daemon-api-approval-rule-kept = { $rule } was written into { $path }, but { $ask_id } was already { $how }, so the ask was not answered; the rule stays, and every later launch carries it.
daemon-api-approval-rule-kept-fix = Take the rule out of { $path } if you did not mean to keep it.
