# n8n Matrix E2EE

Send and receive messages in encrypted Matrix rooms from n8n. A separate
[gateway](https://github.com/spindle-studio/n8n-matrix-e2ee#getting-started)
handles Matrix login and encryption; n8n needs only the gateway URL and API token.

## Install

In your self-hosted n8n instance, open **Settings > Community Nodes** and install:

```text
@spindle-studio/n8n-nodes-matrix-e2ee
```

Start the gateway using the [setup guide](https://github.com/spindle-studio/n8n-matrix-e2ee#getting-started),
then create **Matrix E2EE Gateway API** credentials:

- **Gateway Base URL:** the gateway address reachable from n8n, such as
  `http://n8n-matrix-e2ee-gateway:8080` on a shared container network.
- **API Token:** the gateway's `API_SECRET_TOKEN`.

Test and save the credentials.

## Use the nodes

**Matrix E2EE Trigger** starts a workflow when a text, notice, emote, image, or audio message, reaction, or room invitation (`room.invite`)
arrives. To try it, add the trigger, select your credentials, click
**Listen for test event**, and send a new message or room invite to the bot. Events received
before the trigger connects aren't replayed, and the bot's own events are ignored.

Enable **Download Media** to pass received media to later nodes as binary data.
Choose its output field with **Output Data Field Name**.

**Matrix E2EE** supports two resources:

- **Message:** send text, notices, emotes, images, and Ogg/Opus audio, edit or delete messages, send reactions, or set a typing indicator.
- **Room:** create a room, invite users to a room, join a room by ID or alias, or leave a room.

## Requirements

- Gateway **0.2.0 or later** supporting the same API.
- A current self-hosted n8n release with **Node.js 22.4 or later**.

This package has not been verified for n8n Cloud. Check the
[compatibility table](https://github.com/spindle-studio/n8n-matrix-e2ee#compatibility)
and release notes before upgrading.

## Help

[Report a bug or request a feature](https://github.com/spindle-studio/n8n-matrix-e2ee/issues).
For setup and troubleshooting, see the
[project documentation](https://github.com/spindle-studio/n8n-matrix-e2ee).
Other enquiries: [ben@spindle.studio](mailto:ben@spindle.studio).
