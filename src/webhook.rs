use log::debug;
use crate::bridge::SSE;

pub async fn webhook_worker(sse: &SSE) { // TODO: multiple workers
    if let Some(webhook_store) = &sse.webhook_store {
        let mut rx = webhook_store.signal.subscribe();
        let http_client = reqwest::Client::new();

        loop {
            let _ = rx.recv().await;

            {
                let mut store = webhook_store.store.lock().await;
                let data: Vec<_> = store.drain(..).collect();

                for webhook_data in data {
                    let url = &sse.config.webhook_url.as_ref().unwrap();

                    let endpoint = format!("{}/{}", url, webhook_data.client_id);
                    let mut req = http_client.post(endpoint)
                        .json(&webhook_data);

                    if let Some(auth) = &sse.config.webhook_auth {
                        req = req.header("Authorization", format!("Bearer {}", auth));
                    }

                    let res = req.send().await;

                    match res {
                        Ok(res) => {
                            if res.status().is_success() {
                                debug!("webhook sent to [{}]", url);
                                sse.metrics.lock().await.sent_webhooks.inc();
                            } else {
                                debug!("webhook failed: incorrect status code {}", res.status());
                                sse.metrics.lock().await.failed_webhooks.inc();
                            }
                        }
                        Err(e) => {
                            sse.metrics.lock().await.failed_webhooks.inc();
                            debug!("webhook error: {}", e);
                        }
                    }
                }
            } // release lock and cleanup
        }
    }
}