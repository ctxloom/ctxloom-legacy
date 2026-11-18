# Messages for the crate's errors. Each id is an error's code, which begins with
# the crate's name; the variables are its args.

run-launcher-catch-up = the runs owed a runner could not be read from the store: { $ext_err_txt }
run-launcher-record = a record of the session could not be written: { $ext_err_txt }
run-launcher-fact = the launch could not be recorded: { $reason }
run-launcher-bad-session = { $session } is not a session id
# Step 10c U13 (texts.md §1, a DRAFT): the framing of a live session's
# re-assembled context, handed to its engine at its next event.
run-launcher-context-redelivered = ctxloom: your context changed while this session ran ({ $changed }). What follows replaces the context you were given when the session started; where the two differ, follow this one.
