//! `DeliverHookContext` (10c, the lean hook RPC; owner budget 2026-11-18:
//! under 20 ms added per hook at p95): ONE call answers everything a live
//! session's hook carries, read from the PROJECTIONS with no round awaited.
//!
//! - A primary's pending messages of the urgencies the hook delivers (a
//!   prompt's: informs and wakes), as `setup_application::messages_context`
//!   fits them to the hook's budget, recorded delivered by
//!   `DeliverMessages`. The pending projection is caught up by the round,
//!   so two hooks within one round both read a message pending; the
//!   aggregate records each id once, and a hook gives only what ITS
//!   command recorded, so a message is never delivered twice.
//! - Then what the session is owed, as `RedeliverContext` collects it,
//!   within what the messages left.
//!
//! Both are joined by `setup_application::compose_context`, the one join
//! point of a hook's context.

use angzarr_embedded::v1::{event_page, Cover, DeliveryOutcome, Uuid};
use angzarr_embedded::{Failure, Payload};
use daemon_proto::v1;
use run_ports::Edge;
use session_index::SessionRecord;
use sessions::{DeliverMessages, SessionEvent, SessionName};
use setup_application::{compose_context, messages_context, remaining, HookMessage};

use crate::reactions::budget_of;
use crate::server::Api;
use crate::{ApiError, ApiRefusal};

impl<E: Edge> Api<E> {
    /// `DeliverHookContext`: the messages the hook delivers, then what the
    /// session is owed, joined within the hook's budget. A session the
    /// daemon does not know is refused `SessionNotFound`.
    pub async fn deliver_hook_context(
        &self,
        request: v1::DeliverHookContextRequest,
    ) -> Result<v1::HookContext, ApiError> {
        let name = SessionName::parse(&request.name).map_err(|_| {
            ApiError::from(ApiRefusal::of(&v1::NameInvalid {
                name: request.name.clone(),
                reason: "shape".to_string(),
            }))
        })?;
        // No round awaited: the index as it stands.
        let record = self
            .parts
            .index
            .index()
            .find_by_id(&name.uid())
            .ok_or_else(|| {
                ApiError::from(ApiRefusal::of(&v1::SessionNotFound {
                    name: request.name.clone(),
                }))
            })?;
        let budget = budget_of(request.budget);
        let (messages, delivered) = self.hook_messages(&record, &request, budget).await?;
        // What the messages left of the budget; none left, the owed context
        // waits (it stays owed).
        let used = setup_application::HOOK_CONTEXT_BUDGET - remaining(&[&messages]);
        let left = budget.saturating_sub(used);
        let owed = match left {
            0 => v1::RedeliveredContext::default(),
            left => {
                self.redeliver_context(v1::RedeliverContextRequest {
                    name: request.name.clone(),
                    carried_by: request.carried_by.clone(),
                    budget: u32::try_from(left).unwrap_or(u32::MAX),
                    env: request.env.clone(),
                    cwd: request.cwd.clone(),
                })
                .await?
            }
        };
        Ok(v1::HookContext {
            context: compose_context(&[&messages, &owed.context]),
            delivered,
            changed: owed.changed,
        })
    }

    /// The messages of `request.urgencies` pending for the primary `record`,
    /// within `budget`, once `DeliverMessages` recorded them (with the wake
    /// the prompt is): their context and the ids THIS call recorded. A
    /// child's messages are its turns, so it is given none here.
    async fn hook_messages(
        &self,
        record: &SessionRecord,
        request: &v1::DeliverHookContextRequest,
        budget: usize,
    ) -> Result<(String, Vec<String>), ApiError> {
        let none = (String::new(), Vec::new());
        if record.owner.is_some() {
            return Ok(none);
        }
        let pending: Vec<HookMessage> = {
            let index = self.parts.index.index();
            self.parts
                .pending
                .pending()
                .pending(&record.id)
                .into_iter()
                .map(|message| HookMessage {
                    id: message.message_id,
                    urgency: sessions::urgency_word(message.urgency).to_string(),
                    text: message.text,
                    sender: message
                        .sender
                        .map(|sender| index.name_of_ref(&sender))
                        .unwrap_or_default(),
                })
                .collect()
        };
        let urgencies: Vec<&str> = request.urgencies.iter().map(String::as_str).collect();
        let offered = messages_context(&pending, &urgencies, &record.harp, budget);
        if offered.ids.is_empty() && request.wake_nonce.is_empty() {
            return Ok(none);
        }
        let delivered = self
            .record_delivery(record, &offered.ids, &request.wake_nonce)
            .await?;
        if delivered.is_empty() {
            return Ok(none);
        }
        // What this call recorded, and no other: a message another hook
        // delivered first is left out.
        let given: Vec<HookMessage> = pending
            .into_iter()
            .filter(|message| delivered.contains(&message.id))
            .collect();
        let context = messages_context(&given, &urgencies, &record.harp, budget).context;
        Ok((context, delivered))
    }

    /// `ids` and the wake `nonce` recorded delivered on `record`'s session,
    /// keyed `deliver:<session>:<nonce>:<ids, comma-joined>` (as the
    /// caller's `deliver`): the ids the aggregate recorded (those still
    /// pending; none for a DUPLICATE, which another call recorded).
    async fn record_delivery(
        &self,
        record: &SessionRecord,
        ids: &[String],
        nonce: &str,
    ) -> Result<Vec<String>, ApiError> {
        let root = uuid::Uuid::parse_str(&record.id).map_err(|_| ApiError::Failed {
            detail: format!("the session id {} is not a UUID", record.id),
        })?;
        let cover = Cover {
            domain: sessions::SESSION_DOMAIN.to_string(),
            root: Some(Uuid::from_bytes(*root.as_bytes())),
            edition: None,
        };
        let command = DeliverMessages {
            message_ids: ids.to_vec(),
            wake_nonce: nonce.to_string(),
        };
        let key = format!("deliver:{}:{nonce}:{}", record.id, ids.join(","));
        let answered = match self
            .parts
            .edge
            .handle(sessions::keyed_book(cover, &command, &key))
            .await
        {
            Ok(answered) => answered,
            Err(Failure::Rejected(rejected)) => return Err(ApiRefusal::rejected(&rejected).into()),
            Err(other) => return Err(ApiError::failed(other)),
        };
        if answered.delivery == DeliveryOutcome::Duplicate {
            return Ok(Vec::new());
        }
        Ok(delivered_ids(answered.events.as_ref()))
    }
}

/// The ids every `MessagesDelivered` among `events` names, in order.
fn delivered_ids(events: Option<&angzarr_embedded::v1::EventBook>) -> Vec<String> {
    events
        .into_iter()
        .flat_map(|book| book.pages.iter())
        .filter_map(|page| match &page.body {
            Some(event_page::Body::Event(any)) => SessionEvent::from_any(any).ok(),
            _ => None,
        })
        .filter_map(|event| match event {
            SessionEvent::MessagesDelivered(delivered) => Some(delivered.message_ids),
            _ => None,
        })
        .flatten()
        .collect()
}

#[cfg(test)]
#[path = "hook_context.test.rs"]
mod tests;
