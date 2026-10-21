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

- **Matrix E2EE Trigger:** start a workflow on new messages (text, notices, emotes, images, and voice), reactions, or room membership events (invites, joins, and leaves). Enable **Download Media** to attach received images or audio as binary data for later steps.
- **Matrix E2EE (Message):** send text, notices, emotes, images, and voice messages, reply or post in threads, edit or delete messages, react to messages, or set a typing indicator.
- **Matrix E2EE (Room):** create rooms, join rooms by ID or alias, invite users, or leave rooms.

To test the trigger, add **Matrix E2EE Trigger** to a workflow, select your credentials, click **Listen for test event**, and send a message in a room the Matrix account has joined. Events from before the trigger started listening aren't replayed, and events sent by the connected account itself are ignored.

## Requirements

- Gateway **0.4.0 or later** supporting the same API.
- A current self-hosted n8n release with **Node.js 22.4 or later**.

This package has not been verified for n8n Cloud. Check the
[compatibility table](https://github.com/spindle-studio/n8n-matrix-e2ee#compatibility)
and release notes before upgrading.

## Help

[Report a bug or request a feature](https://github.com/spindle-studio/n8n-matrix-e2ee/issues).
For setup and troubleshooting, see the
[project documentation](https://github.com/spindle-studio/n8n-matrix-e2ee).
Other enquiries: [ben@spindle.studio](mailto:ben@spindle.studio).
