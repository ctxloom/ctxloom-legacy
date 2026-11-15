//! The daemon's `Ctxloom` service (`proto/ctxloom/v1/daemon.proto`)
//! over gRPC, served beside angzarr's coordinator on the daemon's one
//! listener (slice 2 step 5): translation only.
//!
//! - GATE FIRST, EVERYTHING (owner, after step 5): every request to the
//!   service, served RPC or not, is admitted by the user bearer first,
//!   through the same [`UserGate`] as the coordinator ([`Gated`], around
//!   the whole generated server); anything else is the one bare
//!   UNAUTHENTICATED the gate answers, which says nothing of why (S11), nor
//!   whether the RPC is served: a caller that is not the user learns
//!   nothing of the API, not even its shape.
//! - Its use case runs on the runtime's BLOCKING pool (`spawn_blocking`,
//!   M10), never on the two async workers that serve the listener: a slow
//!   use case holds a blocking thread, never the sockets.
//! - A refusal is answered under the gRPC code its `(refusal_status)`
//!   declares, the typed refusal message packed in `google.rpc.Status.
//!   details` (S11); a failure is INTERNAL.
//! - `GetTranscript` and `AssembleContext` are served (slice 3), and
//!   `CreateRun`, `GetRun` and `WatchRun` (U4): a watch's events pumped from
//!   a blocking thread into its stream.
//! - `DeleteSession` and `SweepSessions` are served only in a
//!   `session-destroy` build (M5, the feature of the same name); every other
//!   RPC is the generated default, UNIMPLEMENTED, to the user bearer alone
//!   (a plain build's `DeleteSession` and `SweepSessions` among them).

use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll};

use angzarr_coordinator::{Arrival, Credential, Gate, Unauthenticated};
use angzarr_coordinator_grpc::{bearer, status_of};
use daemon_api::{ApiError, Calls};
use daemon_proto::service::ctxloom_server::{Ctxloom, CtxloomServer};
use daemon_proto::v1;
use prost::Message;
use runs_application::UserGate;
use tonic::body::Body;
use tonic::codegen::tokio_stream::wrappers::ReceiverStream;
use tonic::codegen::{http, BoxFuture, BoxStream, Bytes, Service};
use tonic::metadata::{MetadataMap, MetadataValue};
use tonic::server::NamedService;
use tonic::{Code, Request, Response, Status};

/// How many of a watched run's events wait for its caller to read them.
const WATCH_BUFFER: usize = 16;

/// The service's own name, as a router routes it.
pub use daemon_proto::service::ctxloom_server::SERVICE_NAME;

/// The service, ready to be added to a server: `calls` behind `gate`,
/// every request admitted before it reaches a handler or the generated
/// UNIMPLEMENTED.
pub fn service(calls: Arc<dyn Calls>, gate: UserGate) -> Gated<Warned<CtxloomServer<Served>>> {
    Gated {
        inner: Warned {
            inner: CtxloomServer::new(Served {
                calls: Arc::clone(&calls),
            }),
            calls,
        },
        gate: Arc::new(gate),
    }
}

/// A gRPC service `inner` whose every answer carries the daemon's standing
/// warnings (owner ruling OD-55) as response metadata
/// ([`daemon_proto::WARNING_METADATA`], one prost-encoded `Notice` per
/// value), a refusal's or failure's status too: a command learns them from
/// the answer it already has, never by a call of its own. Behind [`Gated`],
/// so a caller that is not the user is told none. The warnings are read
/// once the answer is made, so a drain's answer carries its hand-over.
#[derive(Clone)]
pub struct Warned<S> {
    inner: S,
    calls: Arc<dyn Calls>,
}

impl<S: NamedService> NamedService for Warned<S> {
    const NAME: &'static str = S::NAME;
}

impl<S> Service<http::Request<Body>> for Warned<S>
where
    S: Service<http::Request<Body>, Response = http::Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        let calls = Arc::clone(&self.calls);
        // As [`Gated`]: the clone made ready is the one called.
        let fresh = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, fresh);
        Box::pin(async move {
            let mut response = inner.call(request).await?;
            warn(response.headers_mut(), &calls.warnings());
            Ok(response)
        })
    }
}

/// A gRPC service `inner` that tells `tell` each request's path (its
/// method, `/ctxloom.v1.Ctxloom/GetDaemon`) as it arrives: the daemon's
/// own record of the calls made of it, for a test under test-support to
/// count them (OD-55: no command asks `GetDaemon` for the warnings).
#[derive(Clone)]
pub struct Told<S> {
    inner: S,
    tell: Arc<dyn Fn(&str) + Send + Sync>,
}

/// `inner`, each request's path told to `tell` ([`Told`]).
pub fn telling<S>(inner: S, tell: Arc<dyn Fn(&str) + Send + Sync>) -> Told<S> {
    Told { inner, tell }
}

impl<S: NamedService> NamedService for Told<S> {
    const NAME: &'static str = S::NAME;
}

impl<S> Service<http::Request<Body>> for Told<S>
where
    S: Service<http::Request<Body>, Response = http::Response<Body>, Error = Infallible>,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        (self.tell)(request.uri().path());
        self.inner.call(request)
    }
}

/// `warnings` added to `headers`, one [`daemon_proto::WARNING_METADATA`]
/// value each, in order.
pub fn warn(headers: &mut http::HeaderMap, warnings: &[v1::Notice]) {
    let mut metadata = MetadataMap::new();
    for warning in warnings {
        metadata.append_bin(
            daemon_proto::WARNING_METADATA,
            MetadataValue::from_bytes(&warning.encode_to_vec()),
        );
    }
    headers.extend(metadata.into_headers());
}

/// The `Ctxloom` service's handlers.
pub struct Served {
    calls: Arc<dyn Calls>,
}

/// A gRPC service `inner` behind the user bearer: a request is handed on
/// only once `gate` admits the bearer it presents; any other (none, another
/// bearer, a malformed `authorization`) is answered the gate's one bare
/// UNAUTHENTICATED, whatever its path names.
#[derive(Clone)]
pub struct Gated<S> {
    inner: S,
    gate: Arc<UserGate>,
}

impl<S: NamedService> NamedService for Gated<S> {
    const NAME: &'static str = S::NAME;
}

impl<S> Service<http::Request<Body>> for Gated<S>
where
    S: Service<http::Request<Body>, Response = http::Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        let gate = Arc::clone(&self.gate);
        // The clone that was made ready is the one called (tower's rule);
        // the one left behind is readied again before the next call.
        let fresh = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, fresh);
        Box::pin(async move {
            let metadata = MetadataMap::from_headers(request.headers().clone());
            if admitted(&gate, &metadata).await {
                inner.call(request).await
            } else {
                Ok(status_of(&Unauthenticated.coded()).into_http())
            }
        })
    }
}

/// Whether `metadata` presents the bearer `gate` admits.
async fn admitted(gate: &UserGate, metadata: &MetadataMap) -> bool {
    let Ok(Some(presented)) = bearer(metadata) else {
        return false;
    };
    let credential = Credential::bearer(presented.secret().as_bytes());
    gate.identify(Arrival::Loopback, Some(&credential))
        .await
        .is_ok()
}

/// `google.rpc.Status`, as googleapis defines it: what a status's details
/// carry.
#[derive(Clone, PartialEq, Message)]
struct RpcStatus {
    #[prost(int32, tag = "1")]
    code: i32,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(message, repeated, tag = "3")]
    details: Vec<prost_types::Any>,
}

/// `error` as the status the caller is answered.
fn status_of_error(error: ApiError) -> Status {
    match error {
        ApiError::Refused(refused) => {
            let code = Code::from_i32(refused.code);
            let message = refused
                .refusal
                .type_url
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string();
            let details = RpcStatus {
                code: refused.code,
                message: message.clone(),
                details: vec![refused.refusal],
            };
            Status::with_details(code, message, Bytes::from(details.encode_to_vec()))
        }
        ApiError::Failed { detail } => Status::internal(detail),
        ApiError::Unimplemented { method } => {
            Status::unimplemented(format!("{method} is not served yet"))
        }
    }
}

impl Served {
    /// Answer `request` (admitted already, by [`Gated`]) with `call`'s use
    /// case, run on the blocking pool (M10).
    async fn answer<T, R, F, Fut>(
        &self,
        request: Request<T>,
        call: F,
    ) -> Result<Response<R>, Status>
    where
        T: Send + 'static,
        R: Send + 'static,
        F: FnOnce(Arc<dyn Calls>, T) -> Fut + Send + 'static,
        Fut: Future<Output = Result<R, ApiError>> + Send,
    {
        let calls = Arc::clone(&self.calls);
        let message = request.into_inner();
        let runtime = tokio::runtime::Handle::current();
        let answered = tokio::task::spawn_blocking(move || runtime.block_on(call(calls, message)))
            .await
            .map_err(|joined| Status::internal(joined.to_string()))?;
        answered.map(Response::new).map_err(status_of_error)
    }
}

#[tonic::async_trait]
impl Ctxloom for Served {
    async fn get_daemon(
        &self,
        request: Request<v1::GetDaemonRequest>,
    ) -> Result<Response<v1::Daemon>, Status> {
        self.answer(request, |calls, _| async move { calls.get_daemon().await })
            .await
    }

    async fn stop_daemon(
        &self,
        request: Request<v1::StopDaemonRequest>,
    ) -> Result<Response<v1::Daemon>, Status> {
        self.answer(request, |calls, message| async move {
            calls.stop_daemon(message).await
        })
        .await
    }

    async fn resolve_project(
        &self,
        request: Request<v1::ResolveProjectRequest>,
    ) -> Result<Response<v1::Project>, Status> {
        self.answer(request, |calls, message| async move {
            calls.resolve_project(message).await
        })
        .await
    }

    async fn get_project(
        &self,
        request: Request<v1::GetProjectRequest>,
    ) -> Result<Response<v1::Project>, Status> {
        self.answer(request, |calls, message| async move {
            calls.get_project(message).await
        })
        .await
    }

    async fn get_session(
        &self,
        request: Request<v1::GetSessionRequest>,
    ) -> Result<Response<v1::Session>, Status> {
        self.answer(request, |calls, message| async move {
            calls.get_session(message).await
        })
        .await
    }

    async fn list_sessions(
        &self,
        request: Request<v1::ListSessionsRequest>,
    ) -> Result<Response<v1::ListSessionsResponse>, Status> {
        self.answer(request, |calls, message| async move {
            calls.list_sessions(message).await
        })
        .await
    }

    async fn search_sessions(
        &self,
        request: Request<v1::SearchSessionsRequest>,
    ) -> Result<Response<v1::ListSessionsResponse>, Status> {
        self.answer(request, |calls, message| async move {
            calls.search_sessions(message).await
        })
        .await
    }

    async fn get_transcript(
        &self,
        request: Request<v1::GetTranscriptRequest>,
    ) -> Result<Response<v1::Transcript>, Status> {
        self.answer(request, |calls, message| async move {
            calls.get_transcript(message).await
        })
        .await
    }

    async fn assemble_context(
        &self,
        request: Request<v1::AssembleContextRequest>,
    ) -> Result<Response<v1::AssembledContext>, Status> {
        self.answer(request, |calls, message| async move {
            calls.assemble_context(message).await
        })
        .await
    }

    async fn create_run(
        &self,
        request: Request<v1::CreateRunRequest>,
    ) -> Result<Response<v1::Run>, Status> {
        self.answer(request, |calls, message| async move {
            calls.create_run(message).await
        })
        .await
    }

    async fn create_hand_off(
        &self,
        request: Request<v1::CreateHandOffRequest>,
    ) -> Result<Response<v1::HandOff>, Status> {
        self.answer(request, |calls, message| async move {
            calls.create_hand_off(message).await
        })
        .await
    }

    async fn get_run(
        &self,
        request: Request<v1::GetRunRequest>,
    ) -> Result<Response<v1::Run>, Status> {
        self.answer(request, |calls, message| async move {
            calls.get_run(message).await
        })
        .await
    }

    async fn delete_hand_off(
        &self,
        request: Request<v1::DeleteHandOffRequest>,
    ) -> Result<Response<v1::HandOff>, Status> {
        self.answer(request, |calls, message| async move {
            calls.delete_hand_off(message).await
        })
        .await
    }

    /// The run's events, pumped from a blocking thread (each waited for
    /// there, M10) into the response stream, until its end or until the
    /// caller is gone; a caller that goes changes nothing of the run (owner
    /// s4-Q7).
    async fn watch_run(
        &self,
        request: Request<v1::WatchRunRequest>,
    ) -> Result<Response<BoxStream<v1::RunEvent>>, Status> {
        let events = self
            .answer(request, |calls, message| async move {
                calls.watch_run(message).await
            })
            .await?
            .into_inner();
        let (tell, heard) = tokio::sync::mpsc::channel(WATCH_BUFFER);
        tokio::task::spawn_blocking(move || {
            for event in events {
                if tell.blocking_send(Ok(event)).is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(heard))))
    }

    #[cfg(feature = "session-destroy")]
    async fn delete_session(
        &self,
        request: Request<v1::DeleteSessionRequest>,
    ) -> Result<Response<v1::DeleteSessionResponse>, Status> {
        self.answer(request, |calls, message| async move {
            calls.delete_session(message).await
        })
        .await
    }

    #[cfg(feature = "session-destroy")]
    async fn sweep_sessions(
        &self,
        request: Request<v1::SweepSessionsRequest>,
    ) -> Result<Response<v1::SweepSessionsResponse>, Status> {
        self.answer(request, |calls, message| async move {
            calls.sweep_sessions(message).await
        })
        .await
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
