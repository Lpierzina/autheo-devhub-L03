# API security boundaries

| Surface | Exposure | Authentication actually implemented |
| --- | --- | --- |
| Marketplace application | Public HTTPS | Marketplace/user application authentication is outside this router |
| DevHub Marketplace API | Private service-to-service | Five-header HMAC-SHA256, timestamp and durable nonce replay protection |
| Hive Admin `/healthz`, `/v1/mesh` | Loopback/private only | **NETWORK ISOLATION / LOOPBACK ONLY** |
| Hive Admin reads | Loopback/private only | Middleware permits reads without a JWT; a presented JWT/API key binds tenant claims |
| Hive Admin mutations | Loopback/private only | JWT/API key only when `HIVE_JWT_SECRET` is configured; handlers add tenant/operator gates |
| Hive Admin platform-operator handlers | Loopback/private only | Handler checks `platform_admin`, derived from the configured owner/admin identity |
| Hive node mesh/relay transport | Mesh/protocol network | Existing Iroh endpoint identity, peer-trust, and relay controls; not HTTP Admin auth |

## Required separation

Marketplace must never call Hive Admin directly. It uses the documented DevHub
HMAC API. DevHub verifies settlement and owns the scheduler integration.
Publishing documentation does not permit publishing the HTTP server.

`hive-cloud` defaults Admin to `127.0.0.1:8786`. It can route an Admin API host
through the public listener only when JWT enforcement is configured; that
mechanism does **not** turn the Admin interface into a supported public API and
must not be used to publish it without a separate reviewed operator boundary.
