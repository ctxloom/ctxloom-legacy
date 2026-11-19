//! The setup writes as the API says them (step 10c U10): a materialize's
//! and an uninstall's reports as their messages, and a materialize's
//! request as the use case's. The daemon says its answers in these; the
//! CLI reads them back ([`super::materialize_report`]); a test serves them
//! in process ([`InProcess`]).

use daemon_proto::v1;
use setup_application::{
    MaterializeRequest, MaterializeResult, Setup, SetupError, SurfaceSpec, UninstallReport,
};

use cli_support::{Exit, ExitClass, Failure};

/// A diagnostic said as the API says a notice: its code, flattened args and
/// en-US text (the caller's flattening: run-application's, in the daemon).
pub type NoticeOf<'a> = &'a dyn Fn(&dyn angzarr_diagnostic::Diagnostic) -> v1::Notice;

/// A materialize's report as the API says it, each failure said by
/// `notice_of` and its text among the errors too.
pub fn materialized(
    result: &MaterializeResult,
    notice_of: NoticeOf<'_>,
) -> v1::MaterializeResponse {
    let failures: Vec<v1::Notice> = result
        .failures
        .iter()
        .map(|failure| notice_of(failure))
        .collect();
    v1::MaterializeResponse {
        target: result.target.clone(),
        created: result.created,
        profiles: result.profiles.clone(),
        engines: result
            .engines
            .iter()
            .map(|engine| v1::MaterializedEngine {
                engine: engine.engine.clone(),
                wrote: engine.wrote.clone(),
                released: engine.released.clone(),
                context_file: engine.context_file.clone(),
                not_carried: engine
                    .not_carried
                    .iter()
                    .map(|lost| v1::NotCarried {
                        surface: lost.surface.clone(),
                        reason: lost.reason.clone(),
                        ..v1::NotCarried::default()
                    })
                    .collect(),
                skipped: engine.skipped.clone(),
                warnings: engine.warnings.clone(),
            })
            .collect(),
        status: result.status.clone(),
        errors: result
            .errors
            .iter()
            .cloned()
            .chain(failures.iter().map(|failure| failure.text.clone()))
            .collect(),
        warnings: result.warnings.clone(),
        failures,
        ..v1::MaterializeResponse::default()
    }
}

/// An uninstall's report as the API says it.
pub fn uninstalled(report: &UninstallReport) -> v1::UninstallManageResponse {
    v1::UninstallManageResponse {
        status: report.status.clone(),
        backends: report.backends.clone(),
        errors: report.errors.clone(),
        note: report.note.clone(),
        ..v1::UninstallManageResponse::default()
    }
}

/// The use case's request a materialize's message asks for, under `flags`.
///
/// # Errors
/// A surface that does not parse.
pub fn materialize_request(
    asked: &v1::MaterializeRequest,
    flags: &library_application::Flags,
) -> Result<MaterializeRequest, SetupError> {
    Ok(MaterializeRequest {
        profiles: asked.profiles.clone(),
        target: (!asked.target.is_empty()).then(|| asked.target.clone()),
        yes: asked.yes,
        engines: asked.backends.clone(),
        surfaces: asked
            .surfaces
            .iter()
            .map(|spec| SurfaceSpec::parse(spec))
            .collect::<Result<_, _>>()?,
        release: asked.release,
        dry_run: asked.dry_run || asked.validate_only,
        leave_launch_context: false,
        flags: flags.clone(),
    })
}

/// `error` in band, as a response carries it: its notice and its exit.
pub fn erred(error: &SetupError, notice_of: NoticeOf<'_>) -> (Option<v1::Notice>, i32) {
    (Some(notice_of(error)), i32::from(error.exit().code(true)))
}

/// The setup writes served by the use cases in this process, writing
/// through their own store: what the CLI is handed in a test.
pub struct InProcess<'a> {
    pub setup: &'a Setup,
    pub notice_of: NoticeOf<'a>,
}

impl super::Foreign for InProcess<'_> {
    fn materialize(
        &self,
        request: v1::MaterializeRequest,
    ) -> Result<v1::MaterializeResponse, Failure> {
        let flags = library_application::Flags::default();
        let done =
            materialize_request(&request, &flags).and_then(|asked| self.setup.materialize(&asked));
        Ok(match done {
            Ok(done) => materialized(&done.value, self.notice_of),
            Err(error) => {
                let (error, exit) = erred(&error, self.notice_of);
                v1::MaterializeResponse {
                    error,
                    exit,
                    ..v1::MaterializeResponse::default()
                }
            }
        })
    }

    fn uninstall(
        &self,
        _: v1::UninstallManageRequest,
    ) -> Result<v1::UninstallManageResponse, Failure> {
        match self.setup.uninstall() {
            Ok(report) => Ok(uninstalled(&report)),
            Err(error) => Err(Failure::new(error, Exit::FAILED)),
        }
    }
}
