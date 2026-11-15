//! Opt-in memo for deterministic tessellation refusals.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// Counts for one solver's body/configuration memo in one context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshDeclineStats {
    /// Lookups finding a remembered decline, without tessellation.
    pub hits: u64,
    /// Requests without a remembered decline, including successful or transient attempts.
    pub misses: u64,
    /// Distinct tolerance keys holding a deterministic decline.
    pub entries: usize,
}

/// Bounded deterministic declines, keyed by exact tolerance bits. The solver owns
/// the enclosing body/content, accuracy, quality and API/solver namespace. This
/// container alone establishes none of those identities and must not be persisted.
///
/// Only the solver that establishes a deterministic refusal may call `remember`.
/// Do not store timeout, cancellation, infrastructure errors, or untyped plugin declines.
/// Concurrent misses may both tessellate; this memo does not provide single-flight work.
#[derive(Default)]
pub struct MeshDeclines {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    reasons: BTreeMap<u64, String>,
    hits: u64,
    misses: u64,
}

impl MeshDeclines {
    /// At most this many tolerances are retained per body/configuration memo.
    pub const MAX_ENTRIES: usize = 64;
    /// Oversized diagnostics are returned normally but are not retained.
    pub const MAX_REASON_BYTES: usize = 4096;

    /// Looks up a decline and counts a hit or miss. The reason is returned verbatim.
    pub fn get(&self, tolerance: f64) -> Option<String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let reason = state.reasons.get(&tolerance.to_bits()).cloned();
        if reason.is_some() {
            state.hits = state.hits.saturating_add(1);
        } else {
            state.misses = state.misses.saturating_add(1);
        }
        reason
    }

    /// Remembers a proven deterministic decline, including its face/tolerance diagnostics.
    /// This does not classify an arbitrary `Attempt::Declined` as deterministic.
    pub fn remember(&self, tolerance: f64, reason: String) {
        if reason.len() > Self::MAX_REASON_BYTES {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let key = tolerance.to_bits();
        if !state.reasons.contains_key(&key) && state.reasons.len() == Self::MAX_ENTRIES {
            state.reasons.pop_first();
        }
        state.reasons.insert(key, reason);
    }

    /// Snapshot without counting another lookup. Counters live as long as this memo.
    pub fn stats(&self) -> MeshDeclineStats {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        MeshDeclineStats { hits: state.hits, misses: state.misses, entries: state.reasons.len() }
    }
}

#[cfg(test)]
mod tests {
    use super::{MeshDeclineStats, MeshDeclines};
    use crate::topo::Body;
    use crate::{Context, Registries};
    use krly3d_core::{Accuracy, Attempt, FakeClock};
    use std::sync::Arc;

    fn memo(cx: &Context, body: &Arc<Body>) -> Arc<MeshDeclines> {
        cx.derived(body, || Attempt::Done(MeshDeclines::default())).into_result().unwrap()
    }

    fn context(accuracy: Accuracy) -> Context {
        Context::new(accuracy, Arc::new(FakeClock::default()), Registries::default())
    }

    #[test]
    fn diagnostics_tolerance_bits_and_counters_are_preserved() {
        let memo = MeshDeclines::default();
        let reason = "face `side#2`: the face needs more than 400000 triangles for a tolerance of 0.22";
        let tolerance = 0.22_f64;
        assert_eq!(memo.stats(), MeshDeclineStats::default());
        assert_eq!(memo.get(tolerance), None);
        memo.remember(tolerance, reason.into());
        assert_eq!(memo.get(tolerance), Some(reason.into()));
        assert_eq!(memo.get(f64::from_bits(tolerance.to_bits() + 1)), None);
        // A second key does not evict the old decline.
        memo.remember(0.3, "other face".into());
        assert_eq!(memo.get(tolerance), Some(reason.into()));
        assert_eq!(memo.stats(), MeshDeclineStats { hits: 2, misses: 2, entries: 2 });
        assert_eq!(memo.stats().hits, 2); // Observing does not count a lookup.
    }

    #[test]
    fn tolerance_retention_is_bounded_and_oversized_reasons_are_not_truncated() {
        let memo = MeshDeclines::default();
        for tolerance in 1..=MeshDeclines::MAX_ENTRIES + 1 {
            memo.remember(tolerance as f64, format!("face `side`: budget at {tolerance}"));
        }
        assert_eq!(memo.stats().entries, MeshDeclines::MAX_ENTRIES);
        assert_eq!(memo.get(1.0), None);
        assert_eq!(memo.get(2.0), Some("face `side`: budget at 2".into()));
        memo.remember(2.0, "replaced reason".into());
        assert_eq!(memo.stats().entries, MeshDeclines::MAX_ENTRIES);
        assert_eq!(memo.get(2.0), Some("replaced reason".into()));
        memo.remember(0.22, "x".repeat(MeshDeclines::MAX_REASON_BYTES + 1));
        assert_eq!(memo.get(0.22), None);
        assert_eq!(memo.stats().entries, MeshDeclines::MAX_ENTRIES);
    }

    #[test]
    fn identity_is_shared_arc_not_independently_allocated_content() {
        let cx = context(Accuracy::default());
        let body = Arc::new(Body::default());
        let original = memo(&cx, &body);
        original.remember(0.22, "budget".into());
        assert!(Arc::ptr_eq(&original, &memo(&cx, &Arc::clone(&body))));
        let same_content = Arc::new(body.as_ref().clone());
        assert_eq!(memo(&cx, &same_content).get(0.22), None);
        let mut changed = body.as_ref().clone();
        changed.solid = !changed.solid;
        assert_eq!(memo(&cx, &Arc::new(changed)).get(0.22), None);
        assert_eq!(original.get(0.22), Some("budget".into()));
    }

    #[test]
    fn editing_a_body_with_only_a_weak_memo_reference_changes_identity() {
        let cx = context(Accuracy::default());
        let mut body = Arc::new(Body::default());
        let original = memo(&cx, &body);
        original.remember(0.22, "budget".into());
        let pointer = Arc::as_ptr(&body);
        Arc::make_mut(&mut body).solid = true;
        assert_ne!(Arc::as_ptr(&body), pointer);
        assert_eq!(memo(&cx, &body).get(0.22), None);
    }

    #[test]
    fn a_new_context_isolates_accuracy_and_solver_configuration() {
        let body = Arc::new(Body::default());
        let first = context(Accuracy::default());
        memo(&first, &body).remember(0.22, "budget".into());
        let new_solver_context = context(Accuracy::default());
        assert_eq!(memo(&new_solver_context, &body).get(0.22), None);
        let accuracy = Accuracy::for_scale(10.0).unwrap().with_resolution(1e-6).unwrap();
        let new_accuracy_context = context(accuracy);
        assert_eq!(memo(&new_accuracy_context, &body).get(0.22), None);
    }

    #[test]
    fn untyped_timeouts_cancellations_and_failures_are_retried_by_mesh_of() {
        use crate::mesh::Mesh;
        use crate::ports::interpret::{mesh_of, MeshRequest, ToMesh};
        use crate::ports::shape::Leaf;
        use crate::{KernelBuilder, Plugin};
        use krly3d_core::{Error, Id};
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Transient {
            calls: Arc<AtomicUsize>,
            first: Attempt<Arc<Mesh>>,
        }
        impl Plugin<ToMesh> for Transient {
            fn id(&self) -> Id {
                Id::new("brep")
            }
            fn call(&self, _: &Context, _: &MeshRequest) -> Attempt<Arc<Mesh>> {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    self.first.clone()
                } else {
                    Attempt::Done(Arc::new(Mesh::default()))
                }
            }
        }
        for message in ["timeout", "cancelled", "infrastructure unavailable"] {
            for first in [Attempt::declined(message), Attempt::Failed(Error::unsupported(message))] {
                let calls = Arc::new(AtomicUsize::new(0));
                let mut builder = KernelBuilder::new();
                builder.plugin::<ToMesh>(Transient { calls: Arc::clone(&calls), first: first.clone() });
                let cx = builder.context();
                let shape = Leaf::of(Arc::new(Body::default()));
                let expected = match first {
                    Attempt::Declined(reason) => {
                        Attempt::declined(format!("mesh reading `brep` declined: {reason}"))
                    }
                    other => other,
                };
                assert_eq!(mesh_of(&cx, &shape, 0.22), expected);
                let success = mesh_of(&cx, &shape, 0.22).into_result().unwrap();
                let again = mesh_of(&cx, &shape, 0.22).into_result().unwrap();
                assert!(Arc::ptr_eq(&success, &again));
                assert_eq!(calls.load(Ordering::SeqCst), 2);
            }
        }
    }
}
