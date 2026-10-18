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


test('mentions are optional and preserved for text, image, and audio events', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });
	for (const message_type of ['text', 'image', 'audio']) {
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
	assert.equal(emitted.length, 15);
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

test('formatted_body is preserved on text events and rejected on image/audio or when malformed', async (t) => {
	const { socket, emitted } = await start(t);
	socket.open();
	socket.message({ type: 'authenticated' });

	socket.message({ ...roomEvent, message_type: 'text', formatted_body: '<p><strong>hello</strong></p>' });
	await delay(0);
	assert.equal(emitted.length, 1);
	assert.equal(emitted[0][0][0].json.formatted_body, '<p><strong>hello</strong></p>');

	for (const invalidEvent of [
		{ ...roomEvent, message_type: 'text', formatted_body: 123 },
		{ ...roomEvent, message_type: 'image', formatted_body: '<b>caption</b>' },
		{ ...roomEvent, message_type: 'audio', formatted_body: '<b>caption</b>' },
	]) {
		socket.message(invalidEvent);
	}
	await delay(0);
	assert.equal(emitted.length, 1);

	socket.message({ ...roomEvent, message_type: 'text' });
	await delay(0);
	assert.equal(emitted.length, 2);
	assert.equal(Object.hasOwn(emitted[1][0][0].json, 'formatted_body'), false);
	assert.equal(Object.hasOwn(emitted[1][0][0].json, 'caption'), false);
});

test('consolidated action node sends text, image, audio, and typing requests', async () => {
	const requests = [];
	const makeContext = (params, binary = {}) => ({
		getInputData: () => [{ json: {}, binary }],
		getCredentials: async () => ({
			baseUrl: 'https://gateway.example.org/',
			apiToken: 'test-secret',
		}),
		getNodeParameter: (name) => params[name],
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
		makeContext(
			{
				operation: 'message',
				roomId: '!room:example.org',
				messageType: 'image',
				binaryPropertyName: 'image',
				caption: '  Alt caption  ',
			},
			{
				image: {
					mimeType: 'image/png',
					fileName: 'photo.png',
				},
			},
		),
	);
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/image',
		qs: {
			room_id: '!room:example.org',
			file_name: 'photo.png',
			body: 'Alt caption',
		},
		headers: {
			'Content-Type': 'image/png',
		},
		body: Buffer.from('binary-bytes'),
		json: true,
	});

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
	assert.deepEqual(requests.at(-1), {
		method: 'POST',
		url: 'https://gateway.example.org/v1/messages/audio',
		qs: {
			room_id: '!room:example.org',
			file_name: 'audio-message.ogg',
			body: 'Voice note',
		},
		headers: {
			'Content-Type': 'audio/ogg',
		},
		body: Buffer.from('binary-bytes'),
		json: true,
	});

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
});
