# n8n Matrix E2EE

Send and receive messages in encrypted Matrix rooms from n8n.

The **gateway** connects to Matrix and stores the bot's session and encryption
keys. The **n8n nodes** connect to the gateway to create, join, invite users to, or leave rooms, send text, notices, emotes, images, audio, and reactions, delete messages or reactions, set typing indicators, or start a workflow when a message, reaction, or room invitation arrives.

## Getting started

You'll need a Matrix bot account, Docker or Podman, and a self-hosted n8n instance.
The gateway image supports Linux AMD64 and ARM64. The commands below use Docker;
replace `docker` with `podman` if that's what you use.

### 1. Configure the gateway

Save [`.env.example`](gateway/.env.example) as `.env` and fill in your details:

```dotenv
HOMESERVER_URL=https://matrix.example.org
MATRIX_USER=@n8n-bot:example.org
MATRIX_PASSWORD=your-bot-password
API_SECRET_TOKEN=your-random-api-token
```

Generate the API token with `openssl rand -hex 32`. Keep `.env` private.

### 2. Start the gateway

From the directory containing `.env`, run:

```sh
docker run --detach --name n8n-matrix-e2ee-gateway \
  --restart unless-stopped \
  --env-file .env \
  --publish 127.0.0.1:8080:8080 \
  --volume n8n-matrix-e2ee-data:/data \
  ghcr.io/spindle-studio/n8n-matrix-e2ee-gateway:0.2.0
```

Keep the data volume when upgrading: it contains the bot's login and encryption
keys. Keep the gateway on a private network and protect its API token.

Check that it has started:

```sh
curl --fail http://localhost:8080/healthz
```

If it isn't ready, check `docker logs n8n-matrix-e2ee-gateway`.

### 3. Connect n8n

In n8n, open **Settings > Community Nodes** and install
`@spindle-studio/n8n-nodes-matrix-e2ee`.

Create **Matrix E2EE Gateway API** credentials with:

- **Gateway Base URL:** `http://localhost:8080` if n8n runs directly on the same host.
- **API Token:** the value of `API_SECRET_TOKEN` in `.env`.

If n8n runs in a container, put both containers on a shared network:

```sh
docker network create n8n-matrix-e2ee
docker network connect n8n-matrix-e2ee n8n-matrix-e2ee-gateway
docker network connect n8n-matrix-e2ee YOUR_N8N_CONTAINER
```

Replace `YOUR_N8N_CONTAINER` with its name, then use
`http://n8n-matrix-e2ee-gateway:8080` as the gateway URL. For n8n on another
machine, provide access over a private network; the command above exposes the
port only on the local host.

Test and save the credentials.

### 4. Receive your first message

Add **Matrix E2EE Trigger** to a workflow, select your credentials, and click
**Listen for test event**. Invite the bot to a room (or join a room with the **Matrix E2EE** `Room > Join` operation) and send a new message.

The trigger receives text, notice, emote, image, and audio messages, reactions, and room invitations (`room.invite`), and can download media for
later steps. It only receives events while listening; earlier events aren't
replayed. Events sent by the bot itself are ignored.

Use **Matrix E2EE** to create, join, invite users to, or leave a room, send, edit, or delete a message, react to a message, or set a typing indicator.

## Compatibility

| n8n package | Gateway |
| --- | --- |
| 0.1.0 | 0.2.0 or later supporting the same API |

Use a current self-hosted n8n release with Node.js 22.4 or later. The package
has not been verified for n8n Cloud. The gateway and nodes have separate versions;
check release notes before upgrading.

## Help and contributions

[Open an issue](https://github.com/spindle-studio/n8n-matrix-e2ee/issues) for bugs
or feature requests. For security issues, follow the [security policy](SECURITY.md).
Other enquiries: [ben@spindle.studio](mailto:ben@spindle.studio).

See [Contributing](CONTRIBUTING.md) to make a change, or the
[development guide](AGENTS.md) for local setup, the API, and Matrix device troubleshooting.

This is an independent community project. Licensed under [MIT](LICENSE).
