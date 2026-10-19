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
	event_type: 'room.message',
	room_id: '!room:example.org',
	sender: '@sender:example.org',
	event_id: '$event',
	body: 'hello',
	message_type: 'text',
};
async function start(t) {
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
		getNodeParameter: (name) => (name === 'downloadMedia' ? false : 'media'),
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
	for (const message_type of ['text', 'notice', 'emote', 'image', 'audio']) {
		for (const mentions of [undefined, { user_ids: [], room: false },
			{ user_ids: ['@ben:example.org'], room: false },
			{ user_ids: [], room: true },
			{ user_ids: ['@ben:example.org'], room: true }]) {
			socket.message({ ...roomEvent, message_type, ...(mentions === undefined ? {} : { mentions }), future_field: true });
			await delay(0);
			const output = emitted.at(-1)[0][0].json;
			assert.equal(output.message_type, message_type);
			assert.deepEqual(output.mentions, mentions);
			assert.equal(Object.hasOwn(output, 'mentions'), mentions !== undefined);
			assert.equal(Object.hasOwn(output, 'future_field'), false);
		}
	}
	assert.equal(emitted.length, 25);
});

test('invalid mention metadata is rejected without breaking subsequent events', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const mentions of [null, [], 'invalid', {}, { user_ids: [], room: 'true' },
		{ user_ids: '@ben:example.org', room: false }, { user_ids: [1], room: false },
		{ user_ids: [' '], room: false }]) {
		socket.message({ ...roomEvent, mentions });
	}
	await delay(0);
	assert.equal(emitted.length, 0);
	socket.message(roomEvent);
	await delay(0);
	assert.equal(emitted.length, 1);
});

test('relates_to is optional and preserved for thread, reply, and edit events', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const message_type of ['text', 'notice', 'emote', 'image', 'audio']) {
		for (const relates_to of [
			undefined,
			{ rel_type: 'thread', event_id: '$root:example.org' },
			{ rel_type: 'reply', event_id: '$reply:example.org' },
			{ rel_type: 'edit', event_id: '$edit:example.org' },
		]) {
			socket.message({
				...roomEvent,
				message_type,
				...(relates_to === undefined ? {} : { relates_to }),
			});
			await delay(0);
			const output = emitted.at(-1)[0][0].json;
			assert.equal(output.message_type, message_type);
			assert.deepEqual(output.relates_to, relates_to);
			assert.equal(Object.hasOwn(output, 'relates_to'), relates_to !== undefined);
		}
	}
	assert.equal(emitted.length, 20);
});

test('invalid relates_to metadata is rejected without breaking subsequent events', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const relates_to of [
		null,
		[],
		'invalid',
		{},
		{ rel_type: 'unknown', event_id: '$abc:example.org' },
		{ rel_type: 'thread', event_id: 'not-an-event-id' },
		{ rel_type: 'reply', event_id: '$' },
		{ rel_type: 'edit', event_id: '$has space' },
	]) {
		socket.message({ ...roomEvent, relates_to });
	}
	await delay(0);
	assert.equal(emitted.length, 0);
	socket.message(roomEvent);
	await delay(0);
	assert.equal(emitted.length, 1);
});

test('formatted_body is preserved on text/notice/emote events and rejected on image/audio or when malformed', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });

	socket.message({ ...roomEvent, message_type: 'text', formatted_body: '<p><strong>hello</strong></p>' });
	socket.message({ ...roomEvent, message_type: 'notice', formatted_body: '<p><em>notice</em></p>' });
	socket.message({ ...roomEvent, message_type: 'emote', formatted_body: '<p>waves <strong>excitedly</strong></p>' });
	await delay(0);
	assert.equal(emitted.length, 3);
	assert.equal(emitted[0][0][0].json.formatted_body, '<p><strong>hello</strong></p>');
	assert.equal(emitted[1][0][0].json.message_type, 'notice');
	assert.equal(emitted[1][0][0].json.formatted_body, '<p><em>notice</em></p>');
	assert.equal(emitted[2][0][0].json.message_type, 'emote');
	assert.equal(emitted[2][0][0].json.formatted_body, '<p>waves <strong>excitedly</strong></p>');

	for (const invalidEvent of [
		{ ...roomEvent, message_type: 'text', formatted_body: 123 },
		{ ...roomEvent, message_type: 'notice', formatted_body: 123 },
		{ ...roomEvent, message_type: 'emote', formatted_body: 123 },
		{ ...roomEvent, message_type: 'image', formatted_body: '<b>caption</b>' },
		{ ...roomEvent, message_type: 'audio', formatted_body: '<b>caption</b>' },
	]) {
		socket.message(invalidEvent);
	}
	await delay(0);
	assert.equal(emitted.length, 3);

	socket.message({ ...roomEvent, message_type: 'text' });
	await delay(0);
	assert.equal(emitted.length, 4);
	assert.equal(Object.hasOwn(emitted[3][0][0].json, 'formatted_body'), false);
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
			name in params ? params[name] : fallback,
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
			room_id: '!room:example.org',
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
			room_id: '!room:example.org',
			body: 'Ping @room and @alice:example.org',
			mentions: {
				user_ids: ['@alice:example.org', '@ben:example.org'],
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
			room_id: '!room:example.org',
			body: 'Silent **notice**',
			mentions: {
				user_ids: ['@alice:example.org'],
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
			room_id: '!room:example.org',
			body: 'waves **hello**',
			mentions: {
				user_ids: ['@alice:example.org'],
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
			room_id: '!room:example.org',
			file_name: 'photo.png',
			body: 'Alt caption',
			mentions: {
				user_ids: ['@alice:example.org'],
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
			room_id: '!room:example.org',
			file_name: 'audio-message.ogg',
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
		/Invalid Matrix user ID/,
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
			room_id: '!room:example.org',
			typing: true,
			timeout_seconds: 25,
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
			room_id: '!room:example.org',
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
			room_id: '!room:example.org',
			body: 'Thread reply',
			relates_to: {
				rel_type: 'thread',
				event_id: '$root:example.org',
			},
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
			room_id: '!room:example.org',
			body: 'Inline notice reply',
			relates_to: {
				rel_type: 'reply',
				event_id: '$target:example.org',
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
			room_id: '!room:example.org',
			file_name: 'diagram.png',
			body: 'Threaded diagram',
			relates_to: {
				rel_type: 'thread',
				event_id: '$root:example.org',
			},
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
			room_id: '!room:example.org',
			file_name: 'audio-message.ogg',
			relates_to: {
				rel_type: 'reply',
				event_id: '$voice:example.org',
			},
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
			room_id: '!room:example.org',
			body: 'Updated **text**',
			mentions: {
				user_ids: ['@alice:example.org'],
				room: false,
			},
			relates_to: {
				rel_type: 'edit',
				event_id: '$original:example.org',
			},
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
			room_id: '!room:example.org',
			body: 'Updated notice',
			relates_to: {
				rel_type: 'edit',
				event_id: '$original_notice:example.org',
			},
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
			room_id: '!room:example.org',
			body: 'waves **again**',
			relates_to: {
				rel_type: 'edit',
				event_id: '$original_emote:example.org',
			},
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
		/Invalid Matrix event ID in Target Event/,
	);
});
