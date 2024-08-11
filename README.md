# rs-bridge ![Docker Pulls](https://img.shields.io/docker/pulls/catcoderr/rs-bridge) ![GitHub License](https://img.shields.io/github/license/swapcoffee/rs-bridge)



A simple, fast, and secure server-sent events bridge
for [TonConnect](https://docs.ton.org/develop/dapps/ton-connect/protocol/bridge) v2 protocol built with Rust.

Brought to you by [swap.coffee](https://swap.coffee) ☕️ ❤️

## Features

- [x] Fully implements the [TonConnect](https://docs.ton.org/develop/dapps/ton-connect/protocol/bridge) SSE bridge
  protocol specification.
- [x] Written in Rust with speed and safety in mind.
- [x] Optimized for low memory usage and high performance.
- [x] Can handle thousands of concurrent connections
- [x] Supports webhooks for wallet notifications.
- [x] Production-ready and battle-tested on [DeWallet](https://t.me/dewallet) with **1.8M** monthly active users.

![img.png](assets/img.png)

## How to run

There is a pre-built Docker image available on [Docker Hub](https://hub.docker.com/r/catcoderr/rs-bridge). You can easily run it with the following command:

```bash
docker run -d --name rs-bridge -p 8080:8080 catcoderr/rs-bridge
```

## Configuration

You can configure the server using environment variables. Here is the list of available options:

| Option                         | Description                                                 | Default Value | Type                |
|---------------------------------|-------------------------------------------------------------|---------------|---------------------|
| **SSE_ENABLE_CORS**             | Enables or disables Cross-Origin Resource Sharing (CORS).    | true          | Boolean             |
| **SSE_MAX_TTL**                 | Maximum Time-To-Live (TTL) for clients.                     | 3600          | Integer             |
| **SSE_MAX_CLIENTS_PER_SUBSCRIBE** | Maximum number of clients allowed per subscription.         | 10            | Integer             |
| **SSE_MAX_PUSHES_PER_SEC**      | Maximum number of pushes allowed per second.                | 5             | Integer             |
| **SSE_HEARTBEAT_SECONDS**       | Interval in seconds for sending heartbeat messages.         | 15            | Integer             |
| **SSE_HEARTBEAT_GROUPS**        | Number of heartbeat groups.                                 | 8             | Integer             |
| **SSE_CLIENT_TTL**              | Time-To-Live (TTL) for clients.                             | 300           | Integer             |
| **SSE_WEBHOOK_URL**             | URL for webhook notifications.                              | None          | String (Optional)   |
| **SSE_WEBHOOK_AUTH**            | Authorization token for webhook notifications.              | None          | String (Optional)   |
| **SSE_BRIDGE_PORT**             | Port for the bridge server.                                 | 8080          | Integer             |
| **SSE_METRICS_PORT**            | Port for the metrics server.                                | 8081          | Integer             |
