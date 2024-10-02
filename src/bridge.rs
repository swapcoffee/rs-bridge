use std::cmp::max;
use std::collections::HashMap;
use std::env;
use std::pin::Pin;
use std::sync::{Arc};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use log::{debug, info};
use serde_json::json;
use tokio::select;
use tokio::sync::{broadcast, Mutex as AsyncMutex, RwLock};
use tokio::time::sleep;
use warp::http::StatusCode;
use warp::reply::{json, with_status};
use warp::sse::Event as SseEvent;
use leaky_bucket::RateLimiter;
use serde::{Deserialize, Serialize};
use crate::metrics::Metrics;
use base64::decoded_len_estimate;
use tokio_stream::{Stream, StreamExt};
use crate::webhook::webhook_worker;

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
            .data(format!(" {}", json.to_string()))
            .id(format!(" {}", self.id))
            .event(" message")
    }
}


#[derive(Clone)]
struct Client {
    signal: broadcast::Sender<()>,
    last_used: Arc<AtomicI64>,
    store: Arc<RwLock<Vec<Event>>>,
    push_limiter: Arc<RateLimiter>,
}

impl Client {
    async fn set_used(&self) {
        self.last_used.store(current_time(), Ordering::Relaxed);
    }

    fn number_of_receivers(&self) -> usize {
        self.signal.receiver_count()
    }
}


#[derive(Clone, Serialize, Deserialize)]
pub struct WebhookData {
    #[serde(skip_serializing)]
    pub client_id: String,
    pub topic: String,
    pub hash: String,
}

pub struct WebhookStore {
    pub signal: broadcast::Sender<()>,
    pub store: AsyncMutex<Vec<WebhookData>>,
}

#[derive(Clone)]
pub struct SSEConfig {
    pub enable_cors: bool,
    pub max_ttl: u32,
    pub max_clients_per_subscribe: usize,
    pub max_pushes_per_sec: u32,
    pub client_ttl: u32,
    pub heartbeat_seconds: u64,
    pub heartbeat_groups: usize,

    pub webhook_url: Option<String>,
    pub webhook_auth: Option<String>,
    
    pub bridge_port: u16,
    pub metrics_port: u16,
}

fn get_env_or_default<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl SSEConfig {
    pub fn create_from_env(prefix: &str) -> Self {
        let enable_cors = get_env_or_default(&format!("{}_ENABLE_CORS", prefix), true);
        let max_ttl = get_env_or_default(&format!("{}_MAX_TTL", prefix), 3600);
        let max_clients_per_subscribe = get_env_or_default(&format!("{}_MAX_CLIENTS_PER_SUBSCRIBE", prefix), 10);
        let max_pushes_per_sec = get_env_or_default(&format!("{}_MAX_PUSHES_PER_SEC", prefix), 5);
        let heartbeat_seconds = get_env_or_default(&format!("{}_HEARTBEAT_SECONDS", prefix), 15);
        let heartbeat_groups = get_env_or_default(&format!("{}_HEARTBEAT_GROUPS", prefix), 8);
        let client_ttl = get_env_or_default(&format!("{}_CLIENT_TTL", prefix), 300); // 5 minutes

        let webhook_url = env::var(format!("{}_WEBHOOK_URL", prefix)).ok();
        let webhook_auth = env::var(format!("{}_WEBHOOK_AUTH", prefix)).ok();
        
        let bridge_port = get_env_or_default(&format!("{}_BRIDGE_PORT", prefix), 8080);
        let metrics_port = get_env_or_default(&format!("{}_METRICS_PORT", prefix), 8081);

        Self {
            enable_cors,
            max_ttl,
            max_clients_per_subscribe,
            max_pushes_per_sec,
            client_ttl,
            heartbeat_seconds,
            heartbeat_groups,
            webhook_url,
            webhook_auth,
            bridge_port,
            metrics_port,
        }
    }

    fn new_client(&self) -> Client {
        let push_limiter = RateLimiter::builder()
            .interval(Duration::from_secs(1))
            .max(self.max_pushes_per_sec as usize)
            .initial(self.max_pushes_per_sec as usize)
            .build();

        let (tx, _rx) = broadcast::channel(100);

        Client {
            signal: tx,
            last_used: Arc::new(AtomicI64::new(current_time())),
            store: Arc::new(RwLock::new(Vec::with_capacity(32))),
            push_limiter: Arc::new(push_limiter),
        }
    }
}


pub struct SSE {
    clients: Arc<AsyncMutex<HashMap<String, Client>>>,
    ping_waiters: Vec<broadcast::Sender<()>>,
    connections_iter: AtomicU64,
    pub config: SSEConfig,
    pub metrics: Arc<Metrics>,
    pub webhook_store: Option<WebhookStore>,
}

fn current_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_nanos() as i64
}

// Helper function for sending events
async fn send_events<'a>(
    clients: &'a [Client],
    last_event_id: &'a mut u64,
    metrics: Arc<Metrics>,
) -> Pin<Box<dyn Stream<Item=Result<SseEvent, warp::Error>> + Send + 'a>> {
    let stream = async_stream::stream! {
            let current_time_seconds = current_time();
        
            for cli in clients {
                let events = cli.store.read().await;
                for e in events.iter() {
                    if e.id > *last_event_id && e.deadline - current_time_seconds >= 0 {
                        metrics.delivered_messages.inc();
                        *last_event_id = max(*last_event_id, e.id);
                        yield Ok(e.to_sse_event());
                    }
                }
            }
        };

    Box::pin(stream)
}


impl SSE {
    pub fn new(config: SSEConfig, metrics: Arc<Metrics>) -> Arc<SSE> {
        let webhook_store = config.webhook_url.as_ref().map(|_url| {
            let (tx, _rx) = broadcast::channel(100);
            WebhookStore {
                signal: tx,
                store: AsyncMutex::new(Vec::new()),
            }
        });

        let mut sse = SSE {
            clients: Arc::new(AsyncMutex::new(HashMap::new())),
            ping_waiters: Vec::with_capacity(config.heartbeat_groups),
            connections_iter: AtomicU64::new(0),
            config,
            metrics,
            webhook_store,
        };

        for _ in 0..sse.config.heartbeat_groups {
            let (sender, _receiver) = broadcast::channel(100);
            sse.ping_waiters.push(sender);
        }

        let sse = Arc::new(sse);
        let sse_pinger = Arc::clone(&sse);
        let sse_cleaner = Arc::clone(&sse);

        tokio::spawn(async move { sse_pinger.ping_worker().await });
        tokio::spawn(async move { sse_cleaner.cleaner_worker().await });

        if sse.config.webhook_url.is_some() {
            let sse_webhook = Arc::clone(&sse);
            tokio::spawn(async move { webhook_worker(&sse_webhook).await });
        }

        sse
    }


    async fn cleaner_worker(&self) {
        loop {
            let now = current_time();

            {
                let mut clients = self.clients.lock().await;

                debug!("clients size before cleanup: {}", clients.len());

                clients.retain(|_id, cli| {
                    let last_used = cli.last_used.load(Ordering::Relaxed);
                    let duration_secs = Duration::from_nanos((now - last_used) as u64).as_secs();
                    let receivers = cli.number_of_receivers();

                    !(duration_secs > self.config.client_ttl as u64 && receivers == 0)
                });

                self.metrics.active_subscriptions.set(clients.len() as f64);

                debug!("clients size after cleanup: {}", clients.len());
            }

            // TODO: configurable?
            sleep(Duration::from_secs(5)).await;
        }
    }

    async fn ping_worker(&self) {
        let per_group = Duration::from_secs(
            self.config.heartbeat_seconds) / self.config.heartbeat_groups as u32;

        loop {
            for i in 0..self.config.heartbeat_groups {
                let sender = self.ping_waiters[i].clone();
                let _ = sender.send(());
                sleep(per_group).await;
            }
        }
    }

    pub(crate) async fn handle_subscribe(
        &self,
        client_ids_str: String,
        last_event_id: Option<u64>,
        host: String,
    ) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
        let mut clients = Vec::new();
        let mut event_receivers = Vec::new();

        let ids: Vec<&str> = client_ids_str.split(',').collect();
        if ids.len() > self.config.max_clients_per_subscribe {
            return Ok(Box::new(with_status(
                json(&json!({
                    "error": "too many client_id passed"
                })),
                StatusCode::BAD_REQUEST,
            )));
        }

        for id in &ids {
            if id.is_empty() || id.len() > 64 {
                return Ok(Box::new(with_status(
                    json(&json!({
                        "error": "invalid client_id"
                    })),
                    StatusCode::BAD_REQUEST,
                )));
            }

            let mut clients_lock = self.clients.lock().await;

            let cli = clients_lock.entry(id.to_string())
                .or_insert_with(|| {
                    SSEConfig::new_client(&self.config)
                }).clone();

            clients.push(cli.clone());
            event_receivers.push(cli.signal.subscribe());
        }

        let ping_shard = self.connections_iter.fetch_add(1, Ordering::Relaxed) % self.config.heartbeat_groups as u64;
        let mut ping_receiver = self.ping_waiters[ping_shard as usize].subscribe();

        info!("subscribed [{}]", client_ids_str);


        self.metrics.requests.with_label_values(&["subscribe", host.as_str()]).observe(1.0);

        let metrics = self.metrics.clone();
        let mut last_event_id = last_event_id.unwrap_or(0);
        let stream = async_stream::stream! {
            
            // send missed events
            {
                let mut event_stream = send_events(&clients, &mut last_event_id, metrics.clone()).await;
                while let Some(event) = event_stream.next().await {
                    yield event;
                }
            }

            loop {
                select! {
                    res = ping_receiver.recv() => {
                        if let Ok(_) = res {
                            info!("ping [{}]", client_ids_str);
                            
                            yield Ok::<_, warp::Error>(SseEvent::default()
                                .event(" heartbeat"));
                        }
                    },
                    res = async {
                        let mut received = None;
                        for rx in &mut event_receivers {
                            if let Ok(val) = rx.recv().await {
                                received = Some(val);
                                break;
                            }
                        }
                        received
                    } => {
                        if let Some(_) = res {
                            info!("events signal [{}]", client_ids_str);
                            
                            // send actual events
                            {
                                let mut event_stream = send_events(&clients, &mut last_event_id, metrics.clone()).await;
                                while let Some(event) = event_stream.next().await {
                                    yield event;
                                }
                            }
                        }
                    }
                }
            }
        };

        let reply = warp::sse::reply(stream);

        Ok(Box::new(reply))
    }

    pub(crate) async fn handle_push(
        &self,
        client_id: String,
        to: String,
        ttl: u64,
        topic: Option<&String>,
        body: Vec<u8>,
        host: String,
    ) -> Result<impl warp::Reply, warp::Rejection> {
        if client_id.is_empty() || client_id.len() > 64 {
            return Ok(with_status(
                json(&json!({
                    "error": "Invalid client_id"
                })),
                StatusCode::BAD_REQUEST,
            ));
        }

        if to.is_empty() || to.len() > 64 {
            return Ok(with_status(
                json(&json!({
                    "error": "Invalid to"
                })),
                StatusCode::BAD_REQUEST,
            ));
        }

        if ttl > self.config.max_ttl as u64 {
            return Ok(with_status(
                json(&json!({
                    "error": "TTL is too long"
                })),
                StatusCode::BAD_REQUEST,
            ));
        }

        let estimated_len = decoded_len_estimate(body.len());

        if estimated_len > 64 * 1024 {
            return Ok(with_status(
                json(&json!({
                    "error": "Too big message"
                })),
                StatusCode::BAD_REQUEST,
            ));
        }

        let mut decode_buffer = vec![0u8; estimated_len];

        let decoded_body = match BASE64_STANDARD.decode_slice(&body, &mut decode_buffer[..]) {
            Ok(decoded_len) => {
                decode_buffer.truncate(decoded_len);
                decode_buffer
            }
            Err(_) => return Ok(with_status(
                json(&json!({
                    "error": "Invalid payload"
                })),
                StatusCode::BAD_REQUEST,
            )),
        };


        let mut clients = self.clients.lock().await;

        let cli = clients.entry(to.clone())
            .or_insert_with(|| {
                SSEConfig::new_client(&self.config)
            }).clone();

        let now_nanos = current_time();

        // TODO: per-ip limits
        if !cli.push_limiter.try_acquire(1) {
            return Ok(with_status(
                json(&json!({
                        "error": "Rate limit exceeded"
                    })),
                StatusCode::TOO_MANY_REQUESTS,
            ));
        }


        let event = Event {
            id: now_nanos as u64,
            from: client_id.clone(),
            message: decoded_body.clone(),
            deadline: now_nanos + Duration::from_secs(ttl).as_nanos() as i64,
        };

        let current_time = current_time();
        let mut events = cli.store.write().await;

        // cleanup expired events
        events.retain(|e| e.deadline - current_time >= 0);

        if events.len() >= 32 {
            return Ok(with_status(
                json(&json!({
                        "error": "Client's buffer overflow"
                    })),
                StatusCode::FORBIDDEN,
            ));
        }

        events.push(event);

        cli.set_used().await;

        debug!("pushed [{}] to [{}]", client_id, to);


        self.metrics.pushed_messages.inc();
        self.metrics.requests.with_label_values(&["push", host.as_str()]).observe(1.0);

        if let (Some(topic), Some(webhook_store)) = (topic, &self.webhook_store) {
            let webhook_data = WebhookData {
                client_id: client_id.clone(),
                topic: topic.clone(),
                hash: decoded_body.iter().map(|b| format!("{:02x}", b)).collect(),
            };

            webhook_store.store.lock().await.push(webhook_data);
            let _ = webhook_store.signal.send(());
        }

        let _ = cli.signal.send(());
        Ok(with_status(
            json(&json!({
                    "status": "OK"
                })),
            StatusCode::OK,
        ))
    }
}
