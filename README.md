# n8n Matrix E2EE

Use Matrix rooms in n8n workflows without giving n8n access to Matrix account
credentials or encryption keys. This project provides:

- a Rust gateway that signs in to Matrix, manages the persistent E2EE store,
  and exposes a small authenticated API; and
- an n8n community node for sending text, images, and Ogg/Opus audio messages,
  managing typing indicators, and triggering workflows from decrypted messages.

The gateway is for a trusted internal network. It is not a public Matrix proxy:
protect its API token, restrict who can reach it, and keep its persistent data
private.

## Getting started

Before starting, you need an existing Matrix homeserver, a dedicated bot account
on that homeserver, Docker, and an n8n instance that can reach the
gateway over the network.

Gateway images support Linux AMD64 and ARM64. Docker selects the matching
architecture automatically; the same commands work on either platform.

### 1. Configure the gateway

Create a directory for the gateway and save
[`.env.example`](gateway/.env.example) there as `.env`. Fill in your homeserver and bot account details:

```dotenv
HOMESERVER_URL=https://matrix.example.org
MATRIX_USER=@n8n-bot:example.org
MATRIX_PASSWORD=your-bot-password
API_SECRET_TOKEN=your-random-api-token
ALLOWED_INVITERS=@you:example.org
```

Generate an API token with `openssl rand -hex 32` and paste it into `.env`.
Keep this file private. `ALLOWED_INVITERS` accepts comma-separated Matrix IDs;
if it is empty, the gateway accepts no invitations. The listening port defaults
to `8080`.

### 2. Start the gateway

Run this from the directory containing `.env`:

```sh
docker run --detach --name n8n-matrix-e2ee-gateway \
  --restart unless-stopped \
  --env-file .env \
  --publish 127.0.0.1:8080:8080 \
  --volume n8n-matrix-e2ee-data:/data \
  ghcr.io/spindle-studio/n8n-matrix-e2ee-gateway:latest
```

Docker creates the persistent volume automatically. Keep it across upgrades:
`/data` holds the bot's login session, device identity, and encryption keys.
For production, pin a published version tag instead of `latest`.

Check startup logs, then confirm the gateway is ready:

```sh
docker logs n8n-matrix-e2ee-gateway
curl --fail http://localhost:8080/healthz
```

For n8n running on the same host, use `http://localhost:8080` as the gateway URL.
If n8n runs in Docker, connect both containers to a shared network:

```sh
docker network create n8n-matrix-e2ee
docker network connect n8n-matrix-e2ee n8n-matrix-e2ee-gateway
docker network connect n8n-matrix-e2ee YOUR_N8N_CONTAINER
```

Then use `http://n8n-matrix-e2ee-gateway:8080` in n8n. Replace
`YOUR_N8N_CONTAINER` with your n8n container's name. For n8n on another machine,
make the gateway reachable through a private network; the command above exposes
it only on the local host.

### 3. Connect n8n

Install `@spindle-studio/n8n-nodes-matrix-e2ee` from
**Settings > Community Nodes** in n8n. See the
[n8n installation guide](https://docs.n8n.io/integrations/community-nodes/installation/)
for other installation methods.

1. Create **Matrix E2EE Gateway API** credentials.
2. Set **Gateway Base URL** to the address n8n uses to reach the gateway.
3. Set **API Token** to the same value as `API_SECRET_TOKEN`.
4. Test and save the credential.
5. Add **Matrix E2EE Trigger** to receive decrypted room messages, or
   **Matrix E2EE** to send a reply.

Invite the bot account to a room from a Matrix ID listed in `ALLOWED_INVITERS`.
The gateway accepts permitted invitations and ignores its own outbound messages,
which prevents workflows from triggering on their replies.

## What the nodes do

**Matrix E2EE** sends text, images from n8n binary data, and Ogg/Opus audio
messages, and typing indicators. **Matrix E2EE Trigger** receives text, image,
and audio messages; it can optionally download received media into an n8n
binary property for downstream processing.

Matrix passwords and encryption keys belong only in the gateway. The n8n
credential contains the gateway URL and API token, not Matrix credentials.

## Compatibility

The node package and gateway are versioned independently. Matching version
numbers are not required; use a gateway compatible with your node version.

| Node version | Gateway compatibility |
| --- | --- |
| 0.1.0 | Gateway 0.1.0 or a later release preserving the required `/v1` API |

Node release notes identify any newer gateway required by new features.

## Project status and support

This is an independent community node, not an official n8n or Matrix project.
Report bugs and feature requests through GitHub. Read
[SECURITY.md](SECURITY.md) before reporting a vulnerability, and
[CONTRIBUTING.md](CONTRIBUTING.md) before sending a change.
For other enquiries, contact [ben@spindle.studio](mailto:ben@spindle.studio).

For architecture, local development, API reference, and Matrix device
troubleshooting, see [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE)
