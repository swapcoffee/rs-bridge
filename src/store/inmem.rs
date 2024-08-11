// in-memory store implementation

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use crate::store::store::{Event, Store};

pub(crate) struct MemoryStore {
    pub events: Mutex<Vec<Arc<Event>>>,
}

impl MemoryStore {
    pub(crate) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
}

impl Store for MemoryStore {
    
    async fn push(&self, event: &Event) -> bool {
        let mut events = self.events.lock().await;

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos() as i64;

        events.retain(|e| e.deadline - current_time >= 0);

        if events.len() < 32 {
            events.push(Arc::new((*event).clone()));
            true
        } else {
            false
        }
    }

    async fn execute_all(&self, last_event_id: u64, mut exec: impl FnMut(&Event) -> ()) {
        let events = self.events.lock().await;
        
        let current_time_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos() as i64;

        for e in events.iter() {
            if e.id > last_event_id && e.deadline - current_time_seconds >= 0 {
                exec(e);
            }
        }
    }
}
