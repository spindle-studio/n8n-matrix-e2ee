const { test } = require('node:test');
const assert = require('node:assert/strict');
const { setTimeout: delay } = require('node:timers/promises');
const { MatrixE2ee } = require('../dist/nodes/MatrixE2ee/MatrixE2ee.node.js');
const { MatrixE2eeTrigger } = require('../dist/nodes/MatrixE2ee/MatrixE2eeTrigger.node.js');

class Socket extends EventTarget {
	static CONNECTING = 0;
	static OPEN = 1;
	static instances = [];
	readyState = 0;
	sent = [];
	constructor(url, ...options) {
		super();
		assert.equal(options.length, 0, 'no bearer headers passed to native WebSocket');
		this.url = url;
		Socket.instances.push(this);
	}
	open() {
		this.readyState = 1;
		this.dispatchEvent(new Event('open'));
	}
	send(data) {
		this.sent.push(JSON.parse(data));
	}
	message(data) {
		this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(data) }));
	}
	close(code = 1000) {
		this.readyState = 3;
		const event = new Event('close');
		event.code = code;
		this.dispatchEvent(event);
	}
}
const roomEvent = {
	eventType: 'message',
	roomId: '!room:example.org',
	sender: '@sender:example.org',
	eventId: '$event',
	body: 'hello',
	messageType: 'text',
};
async function start(t, params = {}) {
	const original = global.WebSocket;
	global.WebSocket = Socket;
	Socket.instances = [];
	const emitted = [],
		failures = [];
	const context = {
		getCredentials: async () => ({
			baseUrl: 'https://gateway.example.org',
			apiToken: 'test-secret',
		}),
		getNodeParameter: (name, fallback) => {
			if (name in params) return params[name];
			if (name === 'downloadMedia') return false;
			if (name === 'binaryPropertyName') return 'media';
			return fallback;
		},
		getNode: () => ({
			name: 'Test trigger',
			type: 'matrixE2eeTrigger',
			typeVersion: 1,
			position: [0, 0],
			parameters: {},
		}),
		emit: (data) => emitted.push(data),
		saveFailedExecution: (error) => failures.push(error),
	};
	const trigger = await MatrixE2eeTrigger.prototype.trigger.call(context);
	t.after(async () => {
		await trigger.closeFunction();
		global.WebSocket = original;
	});
	return { trigger, emitted, failures, socket: Socket.instances[0] };
}
test('sends credentials first and emits only after authentication success', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	assert.deepEqual(socket.sent, [{ type: 'authenticate', token: 'test-secret' }]);
	socket.message({ type: 'authenticated' });
	socket.message(roomEvent);
	await delay(0);
	assert.equal(emitted.length, 1);
	assert.equal(emitted[0][0][0].json.body, 'hello');
});
test('does not emit room events before authentication', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message(roomEvent);
	await delay(0);
	assert.equal(emitted.length, 0);
	assert.equal(socket.readyState, 3);
});
test('authentication rejection reports a safe error without retrying', async (t) => {
	const { socket, failures } = await start(t);
	socket.open();
	socket.close(4401);
	await delay(1100);
	assert.equal(Socket.instances.length, 1);
	assert.equal(failures.length, 1);
	assert.ok(!failures[0].message.includes('test-secret'));
});
test('reconnect authenticates again and shutdown cancels reconnect', async (t) => {
	const { socket, trigger } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	socket.close();
	await delay(1100);
	assert.equal(Socket.instances.length, 2);
	const next = Socket.instances[1];
	next.open();
	assert.deepEqual(next.sent, [{ type: 'authenticate', token: 'test-secret' }]);
	await trigger.closeFunction();
	await delay(1100);
	assert.equal(Socket.instances.length, 2);
});


test('mentions are optional and preserved for text, notice, emote, image, and audio events', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const messageType of ['text', 'notice', 'emote', 'image', 'audio']) {
		for (const mentions of [
			undefined,
			{ userIds: [], room: false },
			{ userIds: ['@ben:example.org'], room: false },
			{ userIds: [], room: true },
			{ userIds: ['@ben:example.org'], room: true },
		]) {
			socket.message({
				...roomEvent,
				messageType,
				...(mentions === undefined ? {} : { mentions }),
				futureField: true,
			});
			await delay(0);
			const output = emitted.at(-1)[0][0].json;
			assert.equal(output.messageType, messageType);
			assert.deepEqual(output.mentions, mentions);
			assert.equal(Object.hasOwn(output, 'mentions'), mentions !== undefined);
			assert.equal(Object.hasOwn(output, 'futureField'), false);
		}
	}
	assert.equal(emitted.length, 25);
});

test('invalid mention metadata is rejected without breaking subsequent events', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const mentions of [
		null,
		[],
		'invalid',
		{},
		{ userIds: [], room: 'true' },
		{ userIds: '@ben:example.org', room: false },
		{ userIds: [1], room: false },
		{ userIds: [' '], room: false },
	]) {
		socket.message({ ...roomEvent, mentions });
	}
	await delay(0);
	assert.equal(emitted.length, 0);
	socket.message(roomEvent);
	await delay(0);
	assert.equal(emitted.length, 1);
});

test('relationType and targetEventId are optional and preserved for thread, reply, and edit events', async (t) => {
	const { socket, emitted } = await start(t, { includeEdits: true });
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const messageType of ['text', 'notice', 'emote', 'image', 'audio']) {
		for (const relation of [
			undefined,
			{ relationType: 'thread', targetEventId: '$root:example.org' },
			{ relationType: 'reply', targetEventId: '$reply:example.org' },
			{ relationType: 'edit', targetEventId: '$edit:example.org' },
		]) {
			socket.message({
				...roomEvent,
				messageType,
				...(relation === undefined ? {} : relation),
			});
			await delay(0);
			const output = emitted.at(-1)[0][0].json;
			assert.equal(output.messageType, messageType);
			assert.equal(output.relationType, relation?.relationType);
			assert.equal(output.targetEventId, relation?.targetEventId);
			assert.equal(Object.hasOwn(output, 'relationType'), relation !== undefined);
			assert.equal(Object.hasOwn(output, 'targetEventId'), relation !== undefined);
		}
	}
	assert.equal(emitted.length, 20);
});

test('edits are ignored by default in On Message mode unless includeEdits is enabled', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });

	socket.message({
		...roomEvent,
		relationType: 'edit',
		targetEventId: '$edit:example.org',
	});
	socket.message({
		...roomEvent,
		relationType: 'reply',
		targetEventId: '$reply:example.org',
	});
	await delay(0);
	assert.equal(emitted.length, 1);
	assert.equal(emitted[0][0][0].json.relationType, 'reply');
});

test('invalid relation metadata is rejected without breaking subsequent events', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const invalidRelation of [
		{ relationType: 'thread' },
		{ targetEventId: '$abc:example.org' },
		{ relationType: 'unknown', targetEventId: '$abc:example.org' },
		{ relationType: 'thread', targetEventId: 'not-an-event-id' },
		{ relationType: 'reply', targetEventId: '$' },
		{ relationType: 'edit', targetEventId: '$has space' },
	]) {
		socket.message({ ...roomEvent, ...invalidRelation });
	}
	await delay(0);
	assert.equal(emitted.length, 0);
	socket.message(roomEvent);
	await delay(0);
	assert.equal(emitted.length, 1);
});

test('reaction events are preserved in On Reaction mode and malformed or non-reaction events are ignored', async (t) => {
	const { socket, emitted } = await start(t, { event: 'reaction' });
	socket.open();
	socket.message({ type: 'authenticated' });

	const reactionEvent = {
		eventType: 'reaction',
		roomId: '!room:example.org',
		sender: '@sender:example.org',
		eventId: '$reaction_event:example.org',
		targetEventId: '$target:example.org',
		key: '👍',
	};
	socket.message(roomEvent);
	socket.message({ ...reactionEvent, futureField: true });
	await delay(0);
	assert.equal(emitted.length, 1);
	assert.deepEqual(emitted[0][0][0].json, reactionEvent);
	assert.equal(emitted[0][0][0].binary, undefined);

	for (const invalidReaction of [
		{ ...reactionEvent, key: '   ' },
		{ ...reactionEvent, targetEventId: undefined },
		{ ...reactionEvent, targetEventId: 'bad-id' },
		{ ...reactionEvent, eventId: 'bad-id' },
	]) {
		socket.message(invalidReaction);
	}
	await delay(0);
	assert.equal(emitted.length, 1);
});

test('room invite, join, and leave events are preserved in On Room Event mode and filtered by roomEvents', async (t) => {
	const { socket, emitted } = await start(t, {
		event: 'room',
		roomEvents: ['invite', 'join', 'leave'],
	});
	socket.open();
	socket.message({ type: 'authenticated' });

	const inviteEvent = {
		eventType: 'invite',
		roomId: '!room:example.org',
		sender: '@inviter:example.org',
		userId: '@bot:example.org',
	};
	const joinEvent = {
		eventType: 'join',
		roomId: '!room:example.org',
		sender: '@alice:example.org',
		userId: '@alice:example.org',
		eventId: '$join:example.org',
	};
	const leaveEvent = {
		eventType: 'leave',
		roomId: '!room:example.org',
		sender: '@mod:example.org',
		userId: '@bob:example.org',
		eventId: '$leave:example.org',
		reason: 'Kicked for spam',
	};

	socket.message(roomEvent);
	socket.message({ ...inviteEvent, futureField: true });
	socket.message(joinEvent);
	socket.message(leaveEvent);
	await delay(0);
	assert.equal(emitted.length, 3);
	assert.deepEqual(emitted[0][0][0].json, inviteEvent);
	assert.deepEqual(emitted[1][0][0].json, joinEvent);
	assert.deepEqual(emitted[2][0][0].json, leaveEvent);

	for (const invalidRoomEvent of [
		{ ...inviteEvent, roomId: '   ' },
		{ ...inviteEvent, sender: '   ' },
		{ ...inviteEvent, userId: '   ' },
		{ ...joinEvent, eventId: 'bad-id' },
		{ ...leaveEvent, eventId: 'bad-id' },
		{ ...leaveEvent, reason: 123 },
	]) {
		socket.message(invalidRoomEvent);
	}
	await delay(0);
	assert.equal(emitted.length, 3);
});

test('room event trigger respects selective roomEvents filter', async (t) => {
	const { socket, emitted } = await start(t, {
		event: 'room',
		roomEvents: ['invite'],
	});
	socket.open();
	socket.message({ type: 'authenticated' });

	socket.message({
		eventType: 'join',
		roomId: '!room:example.org',
		sender: '@alice:example.org',
		userId: '@alice:example.org',
		eventId: '$join:example.org',
	});
	socket.message({
		eventType: 'invite',
		roomId: '!room:example.org',
		sender: '@inviter:example.org',
		userId: '@bot:example.org',
	});
	await delay(0);
	assert.equal(emitted.length, 1);
	assert.equal(emitted[0][0][0].json.eventType, 'invite');
});

test('formattedBody is preserved on text/notice/emote events and rejected on image/audio or when malformed', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });

	socket.message({
		...roomEvent,
		messageType: 'text',
		formattedBody: '<p><strong>hello</strong></p>',
	});
	socket.message({
		...roomEvent,
		messageType: 'notice',
		formattedBody: '<p><em>notice</em></p>',
	});
	socket.message({
		...roomEvent,
		messageType: 'emote',
		formattedBody: '<p>waves <strong>excitedly</strong></p>',
	});
	await delay(0);
	assert.equal(emitted.length, 3);
	assert.equal(emitted[0][0][0].json.formattedBody, '<p><strong>hello</strong></p>');
	assert.equal(emitted[1][0][0].json.messageType, 'notice');
	assert.equal(emitted[1][0][0].json.formattedBody, '<p><em>notice</em></p>');
	assert.equal(emitted[2][0][0].json.messageType, 'emote');
	assert.equal(emitted[2][0][0].json.formattedBody, '<p>waves <strong>excitedly</strong></p>');

	for (const invalidEvent of [
		{ ...roomEvent, messageType: 'text', formattedBody: 123 },
		{ ...roomEvent, messageType: 'notice', formattedBody: 123 },
		{ ...roomEvent, messageType: 'emote', formattedBody: 123 },
		{ ...roomEvent, messageType: 'image', formattedBody: '<b>caption</b>' },
		{ ...roomEvent, messageType: 'audio', formattedBody: '<b>caption</b>' },
	]) {
		socket.message(invalidEvent);
	}
	await delay(0);
	assert.equal(emitted.length, 3);

	socket.message({ ...roomEvent, messageType: 'text' });
	await delay(0);
	assert.equal(emitted.length, 4);
	assert.equal(Object.hasOwn(emitted[3][0][0].json, 'formattedBody'), false);
	assert.equal(Object.hasOwn(emitted[3][0][0].json, 'caption'), false);
});

test('consolidated action node sends text, notice, image, audio, and typing requests', async () => {
	const requests = [];
	const makeContext = (params, binary = {}) => ({
		getInputData: () => [{ json: {}, binary }],
		getCredentials: async () => ({
			baseUrl: 'https://gateway.example.org/',
			apiToken: 'test-secret',
		}),
		getNodeParameter: (name, _index, fallback) =>
			name in params ? params[name] : name === 'resource' ? 'message' : fallback,
		getNode: () => ({
			name: 'Matrix E2EE',
			type: 'matrixE2ee',
			typeVersion: 1,
			position: [0, 0],
			parameters: {},
		}),
		continueOnFail: () => false,
		helpers: {
			assertBinaryData: (_index, propertyName) => {
				const data = binary[propertyName];
				assert.ok(data, `expected binary property ${propertyName}`);
				return data;
			},
			getBinaryDataBuffer: async () => Buffer.from('binary-bytes'),
			httpRequestWithAuthentication: async function (credentialType, options) {
				assert.equal(credentialType, 'matrixE2eeApi');
				requests.push(options);
				return { ok: true };
			},
		},
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'message',
			roomId: '!room:example.org',
			messageType: 'text',
			message: 'Hello **Markdown**',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/text',
		body: {
			roomId: '!room:example.org',
			body: 'Hello **Markdown**',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'message',
			roomId: '!room:example.org',
			messageType: 'text',
			message: 'Ping @room and @alice:example.org',
			additionalFields: {
				roomMention: true,
				userIds: [' @ben:example.org ', '@alice:example.org', '@ben:example.org'],
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/text',
		body: {
			roomId: '!room:example.org',
			body: 'Ping @room and @alice:example.org',
			mentions: {
				userIds: ['@alice:example.org', '@ben:example.org'],
				room: true,
			},
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'message',
			roomId: '!room:example.org',
			messageType: 'notice',
			message: 'Silent **notice**',
			additionalFields: {
				userIds: ['@alice:example.org'],
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/notice',
		body: {
			roomId: '!room:example.org',
			body: 'Silent **notice**',
			mentions: {
				userIds: ['@alice:example.org'],
				room: false,
			},
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'message',
			roomId: '!room:example.org',
			messageType: 'emote',
			message: 'waves **hello**',
			additionalFields: {
				userIds: ['@alice:example.org'],
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/emote',
		body: {
			roomId: '!room:example.org',
			body: 'waves **hello**',
			mentions: {
				userIds: ['@alice:example.org'],
				room: false,
			},
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext(
			{
				operation: 'message',
				roomId: '!room:example.org',
				messageType: 'image',
				binaryPropertyName: 'image',
				caption: '  Alt caption  ',
				additionalFields: {
					roomMention: true,
					userIds: ['@alice:example.org'],
				},
			},
			{
				image: {
					mimeType: 'image/png',
					fileName: 'photo.png',
				},
			},
		),
	);
	{
		const req = requests.at(-1);
		assert.equal(req.method, 'POST');
		assert.equal(req.url, 'https://gateway.example.org/v1/messages/image');
		assert.equal(req.json, true);
		assert.equal(req.qs, undefined);
		assert.ok(req.body instanceof FormData);
		const metadataPart = req.body.get('metadata');
		const mediaPart = req.body.get('media');
		assert.equal(metadataPart.type, 'application/json');
		assert.deepEqual(JSON.parse(await metadataPart.text()), {
			roomId: '!room:example.org',
			fileName: 'photo.png',
			body: 'Alt caption',
			mentions: {
				userIds: ['@alice:example.org'],
				room: true,
			},
		});
		assert.equal(mediaPart.type, 'image/png');
		assert.equal(mediaPart.name, 'photo.png');
		assert.deepEqual(Buffer.from(await mediaPart.arrayBuffer()), Buffer.from('binary-bytes'));
	}

	await MatrixE2ee.prototype.execute.call(
		makeContext(
			{
				operation: 'message',
				roomId: '!room:example.org',
				messageType: 'audio',
				binaryPropertyName: 'audio',
				caption: 'Voice note',
			},
			{
				audio: {
					mimeType: 'audio/ogg',
				},
			},
		),
	);
	{
		const req = requests.at(-1);
		assert.equal(req.method, 'POST');
		assert.equal(req.url, 'https://gateway.example.org/v1/messages/audio');
		assert.equal(req.json, true);
		assert.equal(req.qs, undefined);
		assert.ok(req.body instanceof FormData);
		const metadataPart = req.body.get('metadata');
		const mediaPart = req.body.get('media');
		assert.equal(metadataPart.type, 'application/json');
		assert.deepEqual(JSON.parse(await metadataPart.text()), {
			roomId: '!room:example.org',
			fileName: 'audio-message.ogg',
			body: 'Voice note',
		});
		assert.equal(mediaPart.type, 'audio/ogg');
		assert.equal(mediaPart.name, 'audio-message.ogg');
		assert.deepEqual(Buffer.from(await mediaPart.arrayBuffer()), Buffer.from('binary-bytes'));
	}

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					operation: 'message',
					roomId: '!room:example.org',
					messageType: 'text',
					message: 'Invalid mention',
					additionalFields: {
						userIds: ['not-a-user-id'],
					},
				}),
			),
		/Invalid user ID/,
	);

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'typingIndicator',
			roomId: '!room:example.org',
			typingState: 'start',
			timeoutSeconds: 25,
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/typing',
		body: {
			roomId: '!room:example.org',
			typing: true,
			timeoutSeconds: 25,
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'typingIndicator',
			roomId: '!room:example.org',
			typingState: 'stop',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/typing',
		body: {
			roomId: '!room:example.org',
			typing: false,
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'message',
			roomId: '!room:example.org',
			messageType: 'text',
			message: 'Thread reply',
			relationshipType: 'thread',
			threadRootEventId: ' $root:example.org ',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/text',
		body: {
			roomId: '!room:example.org',
			body: 'Thread reply',
			relationType: 'thread',
			targetEventId: '$root:example.org',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'message',
			roomId: '!room:example.org',
			messageType: 'notice',
			message: 'Inline notice reply',
			relationshipType: 'reply',
			replyTargetEventId: '$target:example.org',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/notice',
		body: {
			roomId: '!room:example.org',
			body: 'Inline notice reply',
			relationType: 'reply',
			targetEventId: '$target:example.org',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext(
			{
				operation: 'message',
				roomId: '!room:example.org',
				messageType: 'image',
				binaryPropertyName: 'image',
				caption: 'Threaded diagram',
				relationshipType: 'thread',
				threadRootEventId: '$root:example.org',
			},
			{
				image: {
					mimeType: 'image/png',
					fileName: 'diagram.png',
				},
			},
		),
	);
	{
		const req = requests.at(-1);
		const metadataPart = req.body.get('metadata');
		assert.deepEqual(JSON.parse(await metadataPart.text()), {
			roomId: '!room:example.org',
			fileName: 'diagram.png',
			body: 'Threaded diagram',
			relationType: 'thread',
			targetEventId: '$root:example.org',
		});
	}

	await MatrixE2ee.prototype.execute.call(
		makeContext(
			{
				operation: 'message',
				roomId: '!room:example.org',
				messageType: 'audio',
				binaryPropertyName: 'audio',
				caption: '',
				relationshipType: 'reply',
				replyTargetEventId: '$voice:example.org',
			},
			{
				audio: {
					mimeType: 'audio/ogg',
				},
			},
		),
	);
	{
		const req = requests.at(-1);
		const metadataPart = req.body.get('metadata');
		assert.deepEqual(JSON.parse(await metadataPart.text()), {
			roomId: '!room:example.org',
			fileName: 'audio-message.ogg',
			relationType: 'reply',
			targetEventId: '$voice:example.org',
		});
	}

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'editMessage',
			roomId: '!room:example.org',
			targetEventId: ' $original:example.org ',
			messageType: 'text',
			message: 'Updated **text**',
			additionalFields: {
				userIds: ['@alice:example.org'],
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/text',
		body: {
			roomId: '!room:example.org',
			body: 'Updated **text**',
			mentions: {
				userIds: ['@alice:example.org'],
				room: false,
			},
			relationType: 'edit',
			targetEventId: '$original:example.org',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'editMessage',
			roomId: '!room:example.org',
			targetEventId: '$original_notice:example.org',
			messageType: 'notice',
			message: 'Updated notice',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/notice',
		body: {
			roomId: '!room:example.org',
			body: 'Updated notice',
			relationType: 'edit',
			targetEventId: '$original_notice:example.org',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'editMessage',
			roomId: '!room:example.org',
			targetEventId: '$original_emote:example.org',
			messageType: 'emote',
			message: 'waves **again**',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/emote',
		body: {
			roomId: '!room:example.org',
			body: 'waves **again**',
			relationType: 'edit',
			targetEventId: '$original_emote:example.org',
		},
		json: true,
	});

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					operation: 'message',
					roomId: '!room:example.org',
					messageType: 'text',
					message: 'Missing thread event ID',
					relationshipType: 'thread',
					threadRootEventId: '   ',
				}),
			),
		/Thread Root Event must not be empty/,
	);

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					operation: 'editMessage',
					roomId: '!room:example.org',
					targetEventId: 'invalid-event-id',
					messageType: 'text',
					message: 'Invalid edit target',
				}),
			),
		/Invalid event ID in Target Event/,
	);

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'reaction',
			roomId: '!room:example.org',
			targetEventId: ' $target_message:example.org ',
			reaction: '👍',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/reaction',
		body: {
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
			key: '👍',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'reaction',
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
			reaction: 'custom',
			customReaction: ' 🦊 ',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/reaction',
		body: {
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
			key: '🦊',
		},
		json: true,
	});

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					operation: 'reaction',
					roomId: '!room:example.org',
					targetEventId: '$target_message:example.org',
					reaction: 'custom',
					customReaction: '   ',
				}),
			),
		/Custom Reaction must not be empty/,
	);

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'redact',
			roomId: '!room:example.org',
			targetEventId: ' $target_message:example.org ',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/redact',
		body: {
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'redact',
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
			additionalFields: {
				reason: '  Spam content  ',
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/redact',
		body: {
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
			reason: 'Spam content',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			operation: 'redact',
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
			additionalFields: {
				reason: '   ',
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/redact',
		body: {
			roomId: '!room:example.org',
			targetEventId: '$target_message:example.org',
		},
		json: true,
	});

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					operation: 'redact',
					roomId: '!room:example.org',
					targetEventId: 'invalid-event-id',
				}),
			),
		/Invalid event ID in Target Event/,
	);

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			resource: 'room',
			operation: 'join',
			roomId: ' #alias:example.org ',
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/room/join',
		body: {
			roomId: '#alias:example.org',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			resource: 'room',
			operation: 'leave',
			roomId: ' #alias:example.org ',
			additionalFields: {
				reason: '  Workflow completed successfully  ',
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/room/leave',
		body: {
			roomId: '#alias:example.org',
			reason: 'Workflow completed successfully',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			resource: 'room',
			operation: 'leave',
			roomId: '!roomid:example.org',
			additionalFields: {
				reason: '   ',
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/room/leave',
		body: {
			roomId: '!roomid:example.org',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			resource: 'room',
			operation: 'create',
			name: ' Incident #402 - Auth Outage ',
			visibility: 'private',
			additionalFields: {
				topic: ' War room for production incident response ',
				roomAliasName: ' incident-402 ',
				invite: [' @bob:example.org ', '', ' @alice:example.org '],
				isEncrypted: true,
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/room/create',
		body: {
			name: 'Incident #402 - Auth Outage',
			visibility: 'private',
			topic: 'War room for production incident response',
			roomAliasName: 'incident-402',
			invite: ['@alice:example.org', '@bob:example.org'],
			isEncrypted: true,
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			resource: 'room',
			operation: 'create',
			name: 'Public Announcements',
			visibility: 'public',
			additionalFields: {
				isEncrypted: false,
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/room/create',
		body: {
			name: 'Public Announcements',
			visibility: 'public',
			isEncrypted: false,
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			resource: 'room',
			operation: 'invite',
			roomId: ' #alias:example.org ',
			userIds: [' @bob:example.org ', '', ' @alice:example.org ', '@bob:example.org'],
			additionalFields: {
				reason: '  Join the incident bridge  ',
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/room/invite',
		body: {
			roomId: '#alias:example.org',
			userIds: ['@alice:example.org', '@bob:example.org'],
			reason: 'Join the incident bridge',
		},
		json: true,
	});

	await MatrixE2ee.prototype.execute.call(
		makeContext({
			resource: 'room',
			operation: 'invite',
			roomId: '!roomid:example.org',
			userIds: '@alice:example.org, @charlie:example.org',
			additionalFields: {
				reason: '   ',
			},
		}),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/room/invite',
		body: {
			roomId: '!roomid:example.org',
			userIds: ['@alice:example.org', '@charlie:example.org'],
		},
		json: true,
	});

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					resource: 'room',
					operation: 'invite',
					roomId: '   ',
					userIds: ['@alice:example.org'],
				}),
			),
		/Room must not be empty/,
	);

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					resource: 'room',
					operation: 'invite',
					roomId: '!room:example.org',
					userIds: ['   '],
				}),
			),
		/Invite Users must contain at least one user ID/,
	);

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					resource: 'room',
					operation: 'invite',
					roomId: '!room:example.org',
					userIds: ['@alice:example.org', 'not-a-user'],
				}),
			),
		/Invalid user ID in Invite Users/,
	);

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					resource: 'room',
					operation: 'join',
					roomId: '   ',
				}),
			),
		/Room must not be empty/,
	);

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					resource: 'room',
					operation: 'leave',
					roomId: '   ',
				}),
			),
		/Room must not be empty/,
	);

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					resource: 'room',
					operation: 'create',
					name: '   ',
					visibility: 'private',
				}),
			),
		/Room Name must not be empty/,
	);

	await assert.rejects(
		() =>
			MatrixE2ee.prototype.execute.call(
				makeContext({
					resource: 'room',
					operation: 'create',
					name: 'Incident Room',
					visibility: 'private',
					additionalFields: {
						invite: '@alice:example.org, invalid-user',
					},
				}),
			),
		/Invalid user ID in Invite Users/,
	);
});
