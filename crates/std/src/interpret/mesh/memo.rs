//! Context-local, bounded buckets indexed by full geometry/configuration bytes.

use super::{identity, Config, Declines};
use krly3d_core::{Attempt, Error};
use krly3d_kernel::topo::Body;
use krly3d_kernel::Context;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

const MAX_BODIES: usize = 64;

#[derive(Default)]
struct ContentDeclines(Mutex<Buckets>);

type Buckets = VecDeque<(Vec<u8>, Arc<Declines>)>;

pub(super) struct Memo {
    pub declines: Arc<Declines>,
    pub cacheable: bool,
}

pub(super) fn memo(cx: &Context, body: &Arc<Body>, config: Config) -> Result<Memo, Error> {
    let Some(key) = identity::key(cx, body, config)? else {
        // Unsupported/oversized bodies still have measurable misses, but cannot
        // retain declines, even when the same allocation is requested again.
        let declines = cx.derived(body, || Attempt::Done(Declines::default())).into_result()?;
        return Ok(Memo { declines, cacheable: false });
    };
    // Context currently exposes body-derived storage only. This permanent empty
    // anchor stores one bounded service per context; it is NOT content identity.
    // All requested-body equality is established by `key`, never by this address.
    static ANCHOR: OnceLock<Arc<Body>> = OnceLock::new();
    let anchor = ANCHOR.get_or_init(|| Arc::new(Body::default()));
    let shared = cx.derived(anchor, || Attempt::Done(ContentDeclines::default())).into_result()?;
    let declines = cx.publish_control(|| {
        let mut buckets = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, memo)) = buckets.iter().find(|(known, _)| *known == key) {
            return Arc::clone(memo);
        }
        if buckets.len() == MAX_BODIES {
            buckets.pop_front();
        }
        let memo = Arc::new(Declines::default());
        buckets.push_back((key, Arc::clone(&memo)));
        memo
    })?;
    Ok(Memo { declines, cacheable: true })
}

#[cfg(test)]
mod tests {
    use super::{memo, Config, MAX_BODIES};
    use crate::tool::testing::context;
    use krly3d_kernel::topo::Body;
    use std::sync::Arc;

    #[test]
    fn context_content_buckets_evict_without_retaining_the_source_allocations() {
        let cx = context();
        let body = Arc::new(Body::default());
        let weak = Arc::downgrade(&body);
        let original = memo(&cx, &body, Config::default()).unwrap();
        original.declines.0.remember(0.22, "first budget".into());
        drop(body);
        assert!(weak.upgrade().is_none());
        let body = Arc::new(Body::default());
        assert_eq!(
            memo(&cx, &body, Config::default()).unwrap().declines.0.get(0.22),
            Some("first budget".into())
        );
        for index in 1..=MAX_BODIES {
            let mut config = Config::default();
            config.quality.triangles += index;
            let other = memo(&cx, &body, config).unwrap();
            assert_eq!(other.declines.0.stats().entries, 0);
        }
        let after_eviction = memo(&cx, &body, Config::default()).unwrap();
        assert!(!Arc::ptr_eq(&original.declines, &after_eviction.declines));
        assert_eq!(after_eviction.declines.0.get(0.22), None);
        // A caller already holding an evicted memo remains safe.
        assert_eq!(original.declines.0.get(0.22), Some("first budget".into()));
    }
}
