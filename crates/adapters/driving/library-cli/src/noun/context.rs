//! `context assemble` (B3): the daemon's `AssembleContext` (step 10a U2),
//! its json the `AssembledContext` (OD-40, design §5.9), its premise index
//! rendered here, at rest (E2: presentation's), into its context.

use crate::prelude::*;

pub(crate) fn read(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let request = v1::AssembleContextRequest {
        profiles: many(sub, "profile"),
        fragments: many(sub, "fragment"),
        tags: many(sub, "tag"),
        globals: cx.globals(),
        ..v1::AssembleContextRequest::default()
    };
    let read = cx.served.assemble_context(request).map(at_rest);
    served(cx, read, out, err, false, |assembled, out, _| {
        writeln!(out, "{}", assembled.context)
    })
}

/// An assembly as `context assemble` shows it: its premise index rendered
/// after its context, at rest (`ctxloom fragment show`), and its notices
/// apart from the message, said as every read's are.
pub(crate) fn at_rest(
    mut assembled: v1::AssembledContext,
) -> (v1::AssembledContext, Vec<v1::Notice>) {
    library_api::premise::render_premise_index(&mut assembled, library_api::premise::Shown::AtRest);
    assembled.premised.clear();
    let notices = std::mem::take(&mut assembled.notices);
    (assembled, notices)
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
