# Messages for the crate's errors. Each id is an error's code, which begins with
# the crate's name; the variables are its args.

# DRAFT (step 8 U4, texts.md §5a): for the owner's approval with the slice's
# other texts (D14).
approvals-approval-unknown = { $ask_id } is no ask that was asked; nothing was answered.
approvals-approval-unknown-fix = List what waits with `ctxloom session approvals`.
approvals-approval-settled = { $ask_id } is already { $how }; nothing was answered.
approvals-option-not-offered = { $ask_id } offers { $offered }, not { $option }; nothing was answered.
approvals-option-not-offered-fix = Answer with one of { $offered }.
approvals-message-not-relayed = { $ask_id } is { $engine }'s ask, and its answer cannot carry a message; nothing was answered.
approvals-message-not-relayed-fix = Answer again without the message.
approvals-name-taken = another ask is named { $ask_id }
