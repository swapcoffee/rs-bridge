use prometheus::{Counter, Gauge, HistogramVec, Opts, Registry};
use std::sync::{Arc};

pub struct Metrics {
    pub active_subscriptions: Gauge,
    pub requests: HistogramVec,
    pub delivered_messages: Counter,
    pub pushed_messages: Counter,
    pub sent_webhooks: Counter,
    pub failed_webhooks: Counter,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        let namespace = "tonconnect";
        let subsystem = "bridge";

        let active_subscriptions = Gauge::with_opts(Opts::new("active_subscriptions", "Active (connected) subscriptions with clients")
            .namespace(namespace)
            .subsystem(subsystem)).unwrap();

        let requests = HistogramVec::new(
            prometheus::HistogramOpts::new("requests", "HTTP Requests")
                .namespace(namespace)
                .subsystem(subsystem),
            &["path", "host"],
        ).unwrap();

        let delivered_messages = Counter::with_opts(Opts::new("delivered_messages", "Delivered messages")
            .namespace(namespace)
            .subsystem(subsystem)).unwrap();

        let pushed_messages = Counter::with_opts(Opts::new("pushed_messages", "Pushed messages")
            .namespace(namespace)
            .subsystem(subsystem)).unwrap();

        let sent_webhooks = Counter::with_opts(Opts::new("sent_webhooks", "Sent webhooks")
            .namespace(namespace)
            .subsystem(subsystem)).unwrap();

        let failed_webhooks = Counter::with_opts(Opts::new("failed_webhooks", "Failed webhooks")
            .namespace(namespace)
            .subsystem(subsystem)).unwrap();

        registry.register(Box::new(active_subscriptions.clone())).unwrap();
        registry.register(Box::new(requests.clone())).unwrap();
        registry.register(Box::new(delivered_messages.clone())).unwrap();
        registry.register(Box::new(pushed_messages.clone())).unwrap();
        registry.register(Box::new(sent_webhooks.clone())).unwrap();
        registry.register(Box::new(failed_webhooks.clone())).unwrap();

        Self {
            active_subscriptions,
            requests,
            delivered_messages,
            pushed_messages,
            sent_webhooks,
            failed_webhooks,
        }
    }
}

pub fn register_metrics(
    registry: &Registry,
) -> Arc<Metrics> {
    let metrics = Metrics::new(&registry);
    Arc::new(metrics)
}
