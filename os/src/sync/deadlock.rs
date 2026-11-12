//! Process-scoped tracking of available units, allocations and pending requests.
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};

/// A resource kind (mutex=0, semaphore=1) and its process-local identifier.
pub type ResourceKey = (u8, usize);

struct Resource {
    available: usize,
    holders: BTreeMap<usize, usize>,
    waiters: VecDeque<usize>,
    handoff: bool,
}

/// Deadlock detection metadata owned by one process.
pub struct DeadlockState {
    enabled: bool,
    resources: BTreeMap<ResourceKey, Resource>,
    retired: BTreeSet<usize>,
}
impl DeadlockState {
    /// Start disabled while keeping accounting active for subsequent enablement.
    pub fn new() -> Self {
        Self {
            enabled: false,
            resources: BTreeMap::new(),
            retired: BTreeSet::new(),
        }
    }
    /// Enable or disable rejection without discarding existing allocations.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Register a resource; FIFO primitives transfer ownership at release time.
    pub fn register(&mut self, key: ResourceKey, count: usize, handoff: bool) {
        self.resources.insert(
            key,
            Resource {
                available: count,
                holders: BTreeMap::new(),
                waiters: VecDeque::new(),
                handoff,
            },
        );
    }

    /// Add one pending request, rejecting and rolling back a deadlocked state.
    pub fn request(&mut self, key: ResourceKey, actor: usize) -> bool {
        let Some(resource) = self.resources.get_mut(&key) else {
            return false;
        };
        if resource.waiters.contains(&actor) || self.retired.contains(&actor) {
            return false;
        }
        resource.waiters.push_back(actor);
        if self.enabled && !self.safe() {
            self.resources
                .get_mut(&key)
                .unwrap()
                .waiters
                .retain(|id| *id != actor);
            return false;
        }
        let resource = self.resources.get_mut(&key).unwrap();
        if resource.available > 0 {
            resource.available -= 1;
            resource.waiters.retain(|id| *id != actor);
            *resource.holders.entry(actor).or_insert(0) += 1;
        }
        true
    }

    /// Record the actual winner of a non-FIFO spin acquisition after it returns.
    pub fn acquired(&mut self, key: ResourceKey, actor: usize) {
        let resource = self.resources.get_mut(&key).unwrap();
        if resource.waiters.contains(&actor) {
            assert!(resource.available > 0);
            resource.available -= 1;
            resource.waiters.retain(|id| *id != actor);
            *resource.holders.entry(actor).or_insert(0) += 1;
        }
    }

    /// Release one unit, enforcing ownership for mutexes but allowing semaphore posts.
    pub fn release(&mut self, key: ResourceKey, actor: usize, owner_required: bool) -> bool {
        let Some(resource) = self.resources.get_mut(&key) else {
            return false;
        };
        if owner_required && !resource.holders.contains_key(&actor) {
            return false;
        }
        if resource.available == usize::MAX {
            return false;
        }
        if let Some(count) = resource.holders.get_mut(&actor) {
            *count -= 1;
            if *count == 0 {
                resource.holders.remove(&actor);
            }
        }
        if resource.handoff {
            if let Some(next) = resource.waiters.pop_front() {
                *resource.holders.entry(next).or_insert(0) += 1;
                return true;
            }
        }
        resource.available += 1;
        true
    }

    /// An exited actor cannot be assumed to run and release outstanding holdings.
    pub fn retire(&mut self, actor: usize) {
        for resource in self.resources.values_mut() {
            resource.waiters.retain(|id| *id != actor);
        }
        if self
            .resources
            .values()
            .any(|res| res.holders.contains_key(&actor))
        {
            self.retired.insert(actor);
        }
    }

    fn safe(&self) -> bool {
        let mut work: BTreeMap<_, _> = self
            .resources
            .iter()
            .map(|(key, res)| (*key, res.available))
            .collect();
        let mut actors = BTreeSet::new();
        for resource in self.resources.values() {
            actors.extend(resource.holders.keys().copied());
            actors.extend(resource.waiters.iter().copied());
        }
        let mut finished = BTreeSet::new();
        loop {
            let mut progress = false;
            for actor in &actors {
                if self.retired.contains(actor) || finished.contains(actor) {
                    continue;
                }
                let runnable = self
                    .resources
                    .iter()
                    .all(|(key, res)| !res.waiters.contains(actor) || work[key] > 0);
                if runnable {
                    for (key, resource) in &self.resources {
                        let held = resource.holders.get(actor).copied().unwrap_or(0);
                        let count = work.get_mut(key).unwrap();
                        *count = count.saturating_add(held);
                    }
                    finished.insert(*actor);
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
        actors
            .iter()
            .all(|id| self.retired.contains(id) || finished.contains(id))
    }
}
