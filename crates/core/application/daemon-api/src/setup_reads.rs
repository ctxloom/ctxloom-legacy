//! The setup reads, served (step 10c U5 and U6, D1): `doctor`, `auth
//! status`, `manage check` and `init prompt`. Each is answered by the setup
//! use cases run where the caller stands (its directory, its environment,
//! its flags under the project's trust), as a process there would run
//! them: the wiring's [`SetupReads`]. No CLI answers one in process, so a
//! command that can reach no daemon diagnoses nothing.

use daemon_proto::v1;
use run_ports::Edge;

use crate::library_writes::LibraryWriteRequest;
use crate::refusal::ApiError;
use crate::server::Api;

/// A setup read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupRead {
    /// `doctor`, `--deps` its own machine's checks alone.
    Doctor {
        deps: bool,
    },
    AuthStatus,
    CheckManage,
    InitPrompt,
}

/// A setup read's answer, as the API says it.
#[derive(Clone, Debug, PartialEq)]
pub enum SetupAnswer {
    Doctor(v1::RunDoctorResponse),
    AuthStatus(v1::GetAuthStatusResponse),
    CheckManage(v1::CheckManageResponse),
    InitPrompt(v1::GetInitPromptResponse),
}

/// The setup use cases that read, each run where a process at `dir`, in
/// `env`, under `flags`, would run it: the wiring's. A use case's own
/// refusal or failure is the call's ([`crate::setup_error`]).
pub trait SetupReads: Send + Sync {
    fn read(
        &self,
        dir: &str,
        env: &[(String, String)],
        flags: &library_application::Flags,
        ask: SetupRead,
    ) -> Result<SetupAnswer, ApiError>;
}

impl<E: Edge> Api<E> {
    /// The setup read `ask` of `request` answered where its caller stands.
    pub(crate) async fn setup_read<Q: LibraryWriteRequest>(
        &self,
        request: &Q,
        ask: SetupRead,
    ) -> Result<SetupAnswer, ApiError> {
        self.open()?;
        let reads = self.setup_reads.as_ref().ok_or_else(|| ApiError::Failed {
            detail: "this daemon answers no setup read".to_string(),
        })?;
        let (site, _) = self
            .resolve(
                request.target(),
                request.cwd(),
                request.env(),
                request.globals(),
            )
            .await?;
        reads.read(&site.at, &site.env, &site.flags, ask)
    }
}

#[cfg(test)]
#[path = "setup_reads.test.rs"]
mod tests;
