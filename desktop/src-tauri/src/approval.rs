use std::{collections::HashMap, sync::Mutex};
use tokio::sync::oneshot;

/// A one-shot decision belongs to exactly one running request.
/// Dropping the receiver or cancelling a run fails closed.
#[derive(Default)]
pub struct ApprovalRegistry {
    pending: Mutex<HashMap<String, PendingApproval>>,
}
struct PendingApproval {
    request_id: String,
    reply: oneshot::Sender<bool>,
}
impl ApprovalRegistry {
    pub fn register(&self, id: String, request_id: String) -> Result<oneshot::Receiver<bool>, String> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().map_err(|_| "Approval state unavailable")?;
        if pending.contains_key(&id) { return Err("Duplicate approval ID".into()); }
        pending.insert(id, PendingApproval { request_id, reply: tx });
        Ok(rx)
    }
    pub fn decide(&self, id: &str, request_id: &str, allow: bool) -> Result<(), String> {
        let mut pending = self.pending.lock().map_err(|_| "Approval state unavailable")?;
        if pending.get(id).map(|p| p.request_id.as_str()) != Some(request_id) {
            return Err("Unknown or mismatched approval".into());
        }
        let item = pending.remove(id).ok_or("Approval expired")?;
        item.reply.send(allow).map_err(|_| "Approval expired".into())
    }
    pub fn cancel_request(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.retain(|_, p| p.request_id != request_id);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn one_shot_and_request_bound() {
        let registry = ApprovalRegistry::default();
        let rx = registry.register("approval".into(), "run".into()).unwrap();
        assert!(registry.decide("approval", "other", true).is_err());
        registry.decide("approval", "run", true).unwrap();
        assert_eq!(rx.await.unwrap(), true);
        assert!(registry.decide("approval", "run", true).is_err());
    }
    #[tokio::test]
    async fn denial_is_delivered_and_cannot_be_replayed() {
        let registry = ApprovalRegistry::default();
        let rx = registry.register("deny-1".into(), "run-1".into()).unwrap();
        registry.decide("deny-1", "run-1", false).unwrap();
        assert_eq!(rx.await.unwrap(), false);
        assert!(registry.decide("deny-1", "run-1", true).is_err());
    }
    #[tokio::test]
    async fn independent_requests_are_isolated() {
        let registry = ApprovalRegistry::default();
        let a = registry.register("a".into(), "run-a".into()).unwrap();
        let b = registry.register("b".into(), "run-b".into()).unwrap();
        registry.cancel_request("run-a");
        assert!(a.await.is_err());
        registry.decide("b", "run-b", true).unwrap();
        assert_eq!(b.await.unwrap(), true);
    }
    #[tokio::test]
    async fn cancellation_denies_pending_approval() {
        let registry = ApprovalRegistry::default();
        let rx = registry.register("approval".into(), "run".into()).unwrap();
        registry.cancel_request("run");
        assert!(rx.await.is_err());
    }
}
