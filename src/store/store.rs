use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::json;
use warp::sse::Event as SseEvent;

#[derive(Serialize, Deserialize, Clone)]
pub struct Event {
    pub id: u64,
    pub deadline: i64,
    pub from: String,
    pub message: Vec<u8>,
}

impl Event {
    pub fn to_sse_event(&self) -> SseEvent {
        let event_data = BASE64_STANDARD.encode(&self.message);
        let json = json!({
                        "from": &self.from,
                        "message": event_data,
                    });

        
        SseEvent::default()
            .data(json.to_string())
            .id(&self.id.to_string())
            .event("message")
    }
}

pub trait Store {
    async fn push(&self, event: &Event) -> bool;

    async fn execute_all(&self, last_event_id: u64, f: impl FnMut(&Event) -> ());
}
