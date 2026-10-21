# Development and implementation guide

This repository contains two deployable parts:

- `gateway`: the Rust service that owns Matrix login,
  E2EE state, room invitation handling, and the authenticated gateway API.
- `nodes`: the npm n8n community-node package that
  connects workflows to the gateway.

The root [README](README.md) is for operators. Keep implementation detail,
developer workflows, API contracts, and troubleshooting here.

## Local development

Requirements: Rust, Node.js 22.4 or later/npm, and Docker or Podman. Copy `gateway/.env.example` to
`.env` and configure a real Matrix bot account, homeserver, and API token. Never commit `.env`, Matrix session files, encryption stores,
or real credentials.

Build and test the gateway:

```sh
cd gateway
cargo test --locked
cargo build --locked --release --package n8n-matrix-e2ee-gateway
```

Run the gateway directly without Docker (loads `.env` automatically; set `DATA_DIR=./data` in `.env` or on the command line for local state persistence):

```sh
cd gateway
DATA_DIR=./data cargo run --locked --package n8n-matrix-e2ee-gateway
```

From the repository root, build its container image and run it with persistent state:

```sh
docker build --tag n8n-matrix-e2ee-gateway gateway
docker volume create gateway-data
docker run --rm --name n8n-matrix-e2ee-gateway \
  --env-file .env --publish 8080:8080 \
  --volume gateway-data:/data n8n-matrix-e2ee-gateway
```

Build and lint the node package:

```sh
cd nodes
npm ci
npm run lint
npm run build
npm test
```

Start the local n8n editor with the package linked and watched for changes:

```sh
cd nodes
N8N_SECURE_COOKIE=false npm run dev
```

Open `http://localhost:5678`. Disabling secure cookies is for local HTTP
development only and allows Safari to sign in; retain secure cookies for HTTPS
deployments. Configure the gateway credential with the host-published gateway
URL and `API_SECRET_TOKEN`. For example, publishing `127.0.0.1:8081:8080`
makes the URL `http://127.0.0.1:8081` for n8n running on the host. Docker
commands above also work with `podman` in place of `docker`.

Click **Listen for test event** on the trigger before sending a Matrix message.
The stream delivers new events only and does not replay messages received before
subscription. Use a separate test bot account, room, API token, and persistent
volume to isolate development from production. Never run concurrent gateways
using the same or copied Matrix session and encryption store. Separate devices
on the same account may both trigger workflows for the same message.

The package has further n8n-specific rules in
[`nodes/AGENTS.md`](nodes/AGENTS.md).
Read its `.agents/` guidance before modifying nodes or credentials.

## Gateway configuration and state

`HOMESERVER_URL`, `MATRIX_USER`, `MATRIX_PASSWORD`, and `API_SECRET_TOKEN` are
required. `PORT` defaults to `8080`. `DATA_DIR`
defaults to `/data` (set `DATA_DIR=./data` when running locally outside a
container).

The `DATA_DIR` directory (`/data` in containers) contains `matrix_session.json` and `matrix_store`.
Keeping both makes the gateway reuse its Matrix device after restart. A new bot
account attempts cross-signing bootstrap at first login. If another Matrix
client reports the device as unverified, inspect gateway startup logs:

1. Check `has_cross_signing_identity` and `is_device_verified`.
2. If no cross-signing identity exists, investigate the homeserver response.
3. If an identity exists but the device is not verified, verify it once from a
   Matrix client or import the applicable secrets.
4. Do not delete the `/data` volume after the device is trusted.

Older gateway volumes without a recoverable session token must be replaced once;
all later restarts should retain the same volume.

## Gateway API

HTTP `/v1` routes require `Authorization: Bearer <API_SECRET_TOKEN>`.
`GET /v1/ws` instead authenticates with the first WebSocket application message.
The API is intentionally private: expose it only to trusted n8n workloads.

| Route | Behavior |
| --- | --- |
| `GET /healthz` | Returns `200` after initialization |
| `GET /v1/auth-check` | Returns `204` for a valid bearer token |
| `POST /v1/messages/text` | Sends `{"roomId":"...","body":"...","mentions":{...},"relationType":"...","targetEventId":"$..."}` as `m.text` (converts Markdown to plain-text `body` and HTML `formatted_body`; `mentions`, `relationType`, and `targetEventId` optional) |
| `POST /v1/messages/notice` | Sends `{"roomId":"...","body":"...","mentions":{...},"relationType":"...","targetEventId":"$..."}` as `m.notice` (converts Markdown to plain-text `body` and HTML `formatted_body` without triggering notifications; `mentions`, `relationType`, and `targetEventId` optional) |
| `POST /v1/messages/emote` | Sends `{"roomId":"...","body":"...","mentions":{...},"relationType":"...","targetEventId":"$..."}` as `m.emote` (converts Markdown to plain-text `body` and HTML `formatted_body`; `mentions`, `relationType`, and `targetEventId` optional) |
| `POST /v1/messages/image` | Sends `multipart/form-data` with a JSON `metadata` part (`roomId`, optional `fileName`, optional `body`, optional `mentions`, optional `relationType` and `targetEventId`) and a binary `media` image part |
| `POST /v1/messages/audio` | Sends `multipart/form-data` with a JSON `metadata` part (`roomId`, optional `fileName`, optional `body`, optional `mentions`, optional `relationType` and `targetEventId`) and a binary `media` Ogg/Opus part as a stable `m.audio` message |
| `POST /v1/messages/reaction` | Sends `{"roomId":"...","targetEventId":"$...","key":"👍"}` as an `m.reaction` (`m.annotation`) event targeting `targetEventId` |
| `POST /v1/messages/redact` | Sends `{"roomId":"...","targetEventId":"$...","reason":"..."}` as an `m.room.redaction` event redacting `targetEventId` (`reason` optional) |
| `POST /v1/room/join` | Joins a room by room ID or alias (`{"roomId":"!room:example.org"}`) and returns `{"roomId":"!room:example.org"}` |
| `POST /v1/room/invite` | Invites one or more users to a joined room by room ID or alias (`{"roomId":"!room:example.org","userIds":["@user:example.org"],"reason":"..."}`, `reason` optional) and returns `{"roomId":"!room:example.org"}` |
| `POST /v1/room/leave` | Leaves a joined or invited room by room ID or alias (`{"roomId":"!room:example.org","reason":"..."}`, `reason` optional) and returns `{"success":true}` |
| `POST /v1/room/create` | Creates a room (`{"name":"...","visibility":"private"|"public","topic":"...","roomAliasName":"...","invite":["@user:example.org"],"isEncrypted":true}`) and returns `{"roomId":"!room:example.org"}` |
| `POST /v1/typing` | Sets `{"roomId":"...","typing":true|false,"timeoutSeconds":30}` (`timeoutSeconds` optional when `typing: true`) |
| `GET /v1/media` | Downloads supported media by `roomId` and `eventId` query parameters |
| `GET /v1/ws` | Streams validated `message`, `reaction`, `invite`, `join`, and `leave` events over WebSocket |

All routes that accept `roomId` accept either a Matrix room ID (`!room:example.org`) or a room alias (`#alias:example.org`).

WebSocket clients must send `{"type":"authenticate","token":"<API_SECRET_TOKEN>"}`
within five seconds. The gateway replies `{"type":"authenticated"}` before
streaming events. Invalid credentials, malformed authentication, and expired
authentication deadlines close with code `4401`. Bearer headers do not authenticate
WebSocket connections. Incoming frames/messages are limited to 4 KiB and at most
64 connections may be awaiting authentication. Clients must wait for the success
response and authenticate again after reconnecting. Use WSS outside trusted
networks; never log authentication payloads. HTTP authentication is unchanged.

The WebSocket stream uses flattened `camelCase` JSON payloads with a top-level `eventType` (`"message"`, `"reaction"`, `"invite"`, `"join"`, or `"leave"`). Message events (`eventType: "message"`) provide text, notice, emote, image, and audio events with `roomId`, `sender`, `eventId`, `body`, and `messageType`; text, notice, and emote events include `formattedBody` when Matrix supplies `org.matrix.custom.html` formatting, and media events can also include `fileName`, `mimeType`, `sizeBytes`, `width`, `height`, and `durationSeconds` metadata. When Matrix supplies `m.mentions`, events also include a `mentions` object:

```json
{"userIds": ["@ben:example.org"], "room": false}
```

`userIds` contains the mentioned Matrix IDs (unique and sorted); `room` indicates
an `@room` mention. Both fields are present when `mentions` is present, defaulting
to `[]` and `false`. Missing Matrix metadata omits `mentions` entirely, preserving
the distinction from an explicitly empty mentions object. Text, notice, emote, image, and audio
messages use the same format. Mentions are never inferred from message text.
This is sender-provided metadata, not proof of notification delivery or permission
to notify the room. The n8n trigger preserves this optional object in its JSON output. Events from
older gateways without `mentions` remain valid. When present, `mentions` must
contain a string array `userIds` and a boolean `room`; malformed metadata is
rejected by the trigger like other invalid event fields.

Outgoing text, notice, emote, image, and audio messages always attach Matrix 1.6+
`m.mentions` metadata (defaulting to `{}` so homeservers and clients do not fall
back to legacy text-matching push rules). Any message request may include an
optional `mentions` object (`{"room": true|false, "userIds": ["@user:example.org"]}`);
`userIds` entries are validated as Matrix user IDs, trimmed, deduplicated, and
sorted.

Message requests also support optional flat `relationType` (`"thread"|"reply"|"edit"`) and `targetEventId` (`"$..."`) fields:

- `thread` (supported for text, notice, emote, image, and audio) injects `m.relates_to` with `rel_type: "m.thread"`, `event_id`, and `m.in_reply_to: {"event_id": "$..."}` into the unencrypted event content before Megolm encryption.
- `reply` (supported for text, notice, emote, image, and audio) injects `m.relates_to` with `m.in_reply_to: {"event_id": "$..."}`.
- `edit` (supported for text, notice, and emote) constructs a Matrix `m.replace` event with `* `-prefixed top-level fallback `body`/`formatted_body`, clean replacement content in `m.new_content`, and `m.relates_to: {"rel_type": "m.replace", "event_id": "$..."}`.

When incoming Matrix messages carry `m.thread`, `m.in_reply_to`, or `m.replace` relations, the WebSocket stream includes top-level `relationType: "thread"|"reply"|"edit"` and `targetEventId: "$..."` (using the clean `m.new_content` body, HTML formatting, and mentions for `m.replace` edits) and the n8n trigger validates and preserves both fields in its JSON output. Incoming `m.reaction` events are streamed with `eventType: "reaction"`, `roomId`, `sender`, `eventId`, `targetEventId`, and `key`. Incoming room invitations for the gateway user are streamed with `eventType: "invite"`, `roomId`, `sender`, `userId`, and optional `reason` so workflows can accept (`POST /v1/room/join`) or reject (`POST /v1/room/leave`) them in n8n. Joined-room membership changes for non-self users are streamed with `eventType: "join"` (`roomId`, `sender`, `userId`, `eventId`) and `eventType: "leave"` (`roomId`, `sender`, `userId`, `eventId`, optional `reason`). The n8n trigger node provides three palette actions (`On Message`, `On Reaction`, and `On Room Event`) to filter events by category, with `On Message` ignoring message edits (`relationType: "edit"`) by default unless `Include Edits` is enabled.

Outgoing audio includes MSC3245 compatibility metadata for a voice marker and
waveform preview. Element X and compatible clients render it as a voice
message, while other clients can fall back to generic audio attachments. The
gateway filters events from its configured Matrix user.

## Branch naming

Use a standard purpose-based prefix and a short lowercase, hyphen-separated
description: `<prefix>/<description>`. Follow an explicitly requested branch name.

Preferred prefixes:

- `feat/`: new features, for example `feat/websocket-authentication`.
- `fix/`: bug fixes, for example `fix/websocket-reconnect`.
- `docs/`: documentation changes, for example `docs/gateway-quickstart`.
- `refactor/`: code restructuring without changing behavior.
- `test/`: test additions or improvements.
- `ci/`: CI and release workflow changes, for example `ci/npm-staged-publishing`.
- `build/`: build tooling, packaging, or dependency changes.
- `chore/`: maintenance that does not fit another category.

Keep `main` as the default branch. Leave release-please's generated branch names
unchanged; do not create manual release branches for its releases.

## Independent releases with release-please

Both components use the same process: merge changes, review the component's
release PR, then merge that PR. Gateway images publish automatically; npm
packages are staged for maintainer approval. Do not bump versions or push release
tags manually, and do not use `n8n-node release`.

| Component | Version source | Release tag | Publication |
| --- | --- | --- | --- |
| Nodes | `nodes/package.json` | `nodes-v<version>` | npm staging with provenance, then maintainer approval |
| Gateway | `gateway/Cargo.toml` | `gateway-v<version>` | GHCR, Linux AMD64 and ARM64 |

`release-please-config.json` defines independent `node` and `rust` strategies
with separate release PRs. `.release-please-manifest.json` records published component versions. Before
any publications it starts empty (`{}`). Initial releases target `0.1.0`; do not
seed the manifest with unreleased versions, which would be treated as previous
releases. Release PRs update the component manifest,
lockfile, `CHANGELOG.md`, and release-please manifest together.

Use Conventional Commits when merging changes into `main`, for example:

- `fix(nodes): handle missing image metadata`
- `feat(gateway): support a new message type`
- `feat(gateway)!: change an API response` for a breaking change

Release-please determines the affected component from changed file paths, not
just the commit scope. A change touching both directories can produce two
release PRs. Root-only documentation or workflow changes do not automatically
release either component. With the configured pre-1.0 policy, fixes increment
the patch, features increment the minor, and breaking changes increment the
minor while the major version is zero. After 1.0, breaking changes increment
that component's major version. A pure `refactor:` or `chore:` does not normally
start a release; the first release needs a releasable commit in the new path.

### One-time setup

1. Host the repository under `spindle-studio` and allow GitHub Actions to create
   pull requests. The release workflow is restricted to this organization.
2. For CI to run automatically on release-please PRs, set `RELEASE_PLEASE_TOKEN`
   to a repository-scoped token with contents, issues, and pull-request write
   access. Without it, release-please uses `GITHUB_TOKEN`, whose generated PRs
   do not trigger CI automatically. In that case, dispatch the **CI** workflow
   on the release branch before merging it. See
   [release-please's token guidance](https://github.com/googleapis/release-please-action#other-actions-on-release-please-prs).
3. Configure npm trusted publishing for owner `spindle-studio`, repository
   `n8n-matrix-e2ee`, and workflow **`release.yml`**. Update an existing trusted
   publisher that points to `publish.yml`. Leave the environment name blank
   and **Allow npm publish** unchecked: the job uses stage-only OIDC trusted
   publishing and does not use `NPM_TOKEN`. Gateway publishing does not depend
   on npm setup. Revoke the bootstrap npm token after validating trusted staging.
4. Allow GitHub Packages publishing. The gateway uses `GITHUB_TOKEN` with
   `packages: write`; after its first publication, set the package visibility
   to **Public** so operators can pull without signing in.

### Releasing

On pushes to `main`, **Release** opens or updates separate release PRs. Review
the version, changelog, and compatibility requirements for each component.
Merge only the release PRs you intend to publish. Release-please then creates
that component's GitHub release and tag. Publishing jobs run in the same
workflow, using the component-specific outputs and checking out the exact tag;
this works even when tags created with `GITHUB_TOKEN` do not trigger workflows.

Before uploading, each publisher validates the tag against its component's
manifest and lockfile. Nodes explicitly run lint, build, and tests, then use
`npm stage publish` with provenance and npm 11.21.0. The gateway runs tests and builds images in parallel on native AMD64 and ARM64
runners. Each platform uses its own GitHub Actions BuildKit cache. A final job
combines the image digests and publishes version/SHA/latest tags only after both
builds succeed. The Dockerfile stays a plain two-stage build; source changes
invalidate the Cargo build layer, so dependency compilation is not cached
separately. Provenance attestations can appear as `unknown/unknown` entries in
the image index; these are metadata, not additional runnable platforms.

After the nodes job succeeds, review the staged package on npmjs.com under
**Staged Packages** and approve it with 2FA. Until approval, the GitHub release
and tag exist but the new package version is not installable from npm. Verify
its version, contents, and provenance before approval. Do not rerun a successful
staging job to approve it: staged versions already reserve their version number.
See [npm staged publishing](https://docs.npmjs.com/staged-publishing/).

Stable npm releases use `latest`; prereleases use `next`. Stable gateway
releases publish the version, full commit SHA, and `latest` image tags;
prereleases publish version and SHA tags only. The default release-please
configuration creates stable releases. Build metadata (`+...`) is excluded
from release versions because Docker tags cannot contain `+`.

If a publisher fails after the GitHub release is created, fix the external
cause and use **Re-run failed jobs** on that run. This preserves the release
job's outputs; rerunning the entire workflow may find no new release to publish.

For a development image, run **Release** manually with `gateway_dev` enabled.
This skips release-please and npm, publishing only a SHA-tagged gateway image.
It never changes version tags or `latest`.

Verify the released gateway manifest with:

```sh
docker buildx imagetools inspect ghcr.io/spindle-studio/n8n-matrix-e2ee-gateway:latest
```

Confirm both `linux/amd64` and `linux/arm64`, verify an unauthenticated pull, and
run the README quickstart with a dedicated bot account.

Keep changes backward compatible with the `/v1` HTTP and WebSocket contract.
When a node feature needs a newer gateway, update the root README compatibility
table and the node release notes. Do not release the other component solely to
match version numbers.

Release validation tests require Python 3.11+:

```sh
python3 -m unittest discover -s .github/scripts -p 'test_*.py'
```

### Node verification status

The package enables the unmodified `@n8n/node-cli` strict ESLint configuration
and has no external runtime dependencies. The trigger uses Node's built-in
WebSocket and the abortable `sleep` helper from `n8n-workflow`. Minimum supported
versions are Node.js 22.4 and `n8n-workflow` 2.35.3 (the validated helper version).
The strict rules accept the built-in WebSocket API. The published verification
guidelines do not explicitly approve that transport, however: passing automated
checks is not a guarantee of Creator Portal approval or Cloud runtime support.
Do not describe the package as verified until n8n approves it.

This protocol change requires a breaking gateway release (0.2.0 under the
pre-1.0 policy) before publishing nodes 0.1.0. Release-please still owns version
bumps. Do not merge the existing nodes release PR before this change and the
compatible gateway release are available.
