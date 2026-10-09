# n8n Matrix E2EE

An n8n community node for Matrix rooms protected by end-to-end encryption. It
connects n8n to the Matrix E2EE gateway; the gateway, not n8n, owns Matrix
credentials, session state, and encryption keys.

## Installation

Install `@spindle-studio/n8n-nodes-matrix-e2ee` using the
[n8n community-node installation guide](https://docs.n8n.io/integrations/community-nodes/installation/).
The gateway must be running and reachable from n8n before configuring the node.

## Nodes

- **Matrix E2EE** sends text, images from incoming binary data, Ogg/Opus audio
  messages, and typing indicators.
- **Matrix E2EE Trigger** starts workflows for decrypted text, image, and audio
  messages. It can download media into a binary property.

## Credentials

Create **Matrix E2EE Gateway API** credentials with:

- **Gateway Base URL**: the HTTP URL n8n uses to reach the gateway. In a shared
  container network this might be `http://n8n-matrix-e2ee-gateway:8080`.
- **API Token**: the gateway's `API_SECRET_TOKEN`.

Do not enter a Matrix user ID or password in n8n. See the
[gateway setup and security guide](https://github.com/spindle-studio/n8n-matrix-e2ee#3-connect-n8n)
for deployment, invitation allowlisting, persistent storage, and troubleshooting.

## Compatibility

The node package and gateway are versioned independently. See the
[gateway compatibility table](https://github.com/spindle-studio/n8n-matrix-e2ee#compatibility)
for supported combinations and check node release notes for new requirements.

Use a current n8n release that supports API version 1 community nodes. Outgoing
audio uses the stable `m.audio` message type with MSC3245 compatibility
metadata. Element X and compatible clients render it as a voice message with a
waveform preview; other clients can fall back to generic audio attachments.

## Resources

- [Project documentation](https://github.com/spindle-studio/n8n-matrix-e2ee)
- [n8n community nodes](https://docs.n8n.io/integrations/community-nodes/)

For project enquiries, contact [ben@spindle.studio](mailto:ben@spindle.studio).
