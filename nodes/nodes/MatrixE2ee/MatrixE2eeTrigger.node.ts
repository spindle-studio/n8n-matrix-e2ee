import type {
	IBinaryData,
	IDataObject,
	INodeExecutionData,
	INodeType,
	INodeTypeDescription,
	ITriggerFunctions,
	ITriggerResponse,
} from 'n8n-workflow';
import { NodeConnectionTypes, NodeOperationError, sleep } from 'n8n-workflow';

const INITIAL_RECONNECT_DELAY_MS = 1_000;
const MAX_RECONNECT_DELAY_MS = 30_000;
const AUTH_TIMEOUT_MS = 10_000;

type MatrixE2eeMessageType = 'text' | 'notice' | 'emote' | 'image' | 'audio';
type MatrixE2eeRelationshipType = 'thread' | 'reply' | 'edit';
type MatrixE2eeTriggerEventMode = 'message' | 'reaction' | 'room';
type MatrixE2eeRoomEventType = 'invite' | 'join' | 'leave';

interface MatrixE2eeMentions extends IDataObject {
	userIds: string[];
	room: boolean;
}

interface MatrixE2eeMessageEvent extends IDataObject {
	eventType: 'message';
	roomId: string;
	sender: string;
	eventId: string;
	body: string;
	formattedBody?: string;
	messageType: MatrixE2eeMessageType;
	mentions?: MatrixE2eeMentions;
	relationType?: MatrixE2eeRelationshipType;
	targetEventId?: string;
	fileName?: string;
	mimeType?: string;
	sizeBytes?: number;
	width?: number;
	height?: number;
	durationSeconds?: number;
}

interface MatrixE2eeReactionEvent extends IDataObject {
	eventType: 'reaction';
	roomId: string;
	sender: string;
	eventId: string;
	targetEventId: string;
	key: string;
}

interface MatrixE2eeInviteEvent extends IDataObject {
	eventType: 'invite';
	roomId: string;
	sender: string;
	userId: string;
	reason?: string;
}

interface MatrixE2eeJoinEvent extends IDataObject {
	eventType: 'join';
	roomId: string;
	sender: string;
	userId: string;
	eventId: string;
}

interface MatrixE2eeLeaveEvent extends IDataObject {
	eventType: 'leave';
	roomId: string;
	sender: string;
	userId: string;
	eventId: string;
	reason?: string;
}

type MatrixE2eeEvent =
	| MatrixE2eeMessageEvent
	| MatrixE2eeReactionEvent
	| MatrixE2eeInviteEvent
	| MatrixE2eeJoinEvent
	| MatrixE2eeLeaveEvent;

export function getGatewayWebSocketUrl(baseUrl: string): string {
	const parsedUrl = new URL(baseUrl.trim());
	const wsProtocol =
		parsedUrl.protocol === 'https:' ? 'wss:' : parsedUrl.protocol === 'http:' ? 'ws:' : '';
	if (wsProtocol.length === 0) {
		return '';
	}

	parsedUrl.protocol = wsProtocol;
	parsedUrl.pathname = '/v1/ws';
	parsedUrl.search = '';
	parsedUrl.hash = '';

	return parsedUrl.toString();
}

export function getGatewayMediaUrl(baseUrl: string): string {
	const parsedUrl = new URL(baseUrl.trim());
	parsedUrl.pathname = '/v1/media';
	parsedUrl.search = '';
	parsedUrl.hash = '';
	return parsedUrl.toString();
}

function isNonEmptyString(value: unknown): value is string {
	return typeof value === 'string' && value.trim().length > 0;
}

function isOptionalString(value: unknown): value is string | undefined {
	return value === undefined || typeof value === 'string';
}

function isOptionalNonNegativeNumber(value: unknown): value is number | undefined {
	return value === undefined || (typeof value === 'number' && Number.isFinite(value) && value >= 0);
}

function isMentions(value: unknown): value is MatrixE2eeMentions {
	if (typeof value !== 'object' || value === null || Array.isArray(value)) {
		return false;
	}
	const mentions = value as Record<string, unknown>;
	return (
		Array.isArray(mentions.userIds) &&
		mentions.userIds.every(isNonEmptyString) &&
		typeof mentions.room === 'boolean'
	);
}

function isValidEventId(value: unknown): value is string {
	return (
		typeof value === 'string' &&
		value.startsWith('$') &&
		value.length > 1 &&
		!/\s/.test(value)
	);
}

function isRelationshipType(value: unknown): value is MatrixE2eeRelationshipType {
	return value === 'thread' || value === 'reply' || value === 'edit';
}

function normalizeMatrixE2eeEvent(payload: unknown): MatrixE2eeEvent | undefined {
	if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
		return undefined;
	}

	const event = payload as Record<string, unknown>;

	if (event.eventType === 'invite') {
		if (
			!['roomId', 'sender', 'userId'].every((field) => isNonEmptyString(event[field])) ||
			!isOptionalString(event.reason)
		) {
			return undefined;
		}
		const reason = event.reason?.trim() || undefined;

		return {
			eventType: 'invite',
			roomId: event.roomId as string,
			sender: event.sender as string,
			userId: event.userId as string,
			...(reason === undefined ? {} : { reason }),
		};
	}

	if (event.eventType === 'join') {
		if (
			!['roomId', 'sender', 'userId'].every((field) => isNonEmptyString(event[field])) ||
			!isValidEventId(event.eventId)
		) {
			return undefined;
		}

		return {
			eventType: 'join',
			roomId: event.roomId as string,
			sender: event.sender as string,
			userId: event.userId as string,
			eventId: event.eventId,
		};
	}

	if (event.eventType === 'leave') {
		if (
			!['roomId', 'sender', 'userId'].every((field) => isNonEmptyString(event[field])) ||
			!isValidEventId(event.eventId) ||
			!isOptionalString(event.reason)
		) {
			return undefined;
		}
		const reason = event.reason?.trim() || undefined;

		return {
			eventType: 'leave',
			roomId: event.roomId as string,
			sender: event.sender as string,
			userId: event.userId as string,
			eventId: event.eventId,
			...(reason === undefined ? {} : { reason }),
		};
	}

	if (event.eventType === 'reaction') {
		if (
			!['roomId', 'sender', 'key'].every((field) => isNonEmptyString(event[field])) ||
			!isValidEventId(event.eventId) ||
			!isValidEventId(event.targetEventId)
		) {
			return undefined;
		}

		return {
			eventType: 'reaction',
			roomId: event.roomId as string,
			sender: event.sender as string,
			eventId: event.eventId,
			targetEventId: event.targetEventId,
			key: event.key as string,
		};
	}

	if (
		event.eventType !== 'message' ||
		!['roomId', 'sender', 'eventId', 'body'].every((field) => isNonEmptyString(event[field]))
	) {
		return undefined;
	}

	const messageType = event.messageType;
	if (
		messageType !== undefined &&
		messageType !== 'text' &&
		messageType !== 'notice' &&
		messageType !== 'emote' &&
		messageType !== 'image' &&
		messageType !== 'audio'
	) {
		return undefined;
	}
	const normalizedMessageType: MatrixE2eeMessageType =
		messageType === 'notice' ||
		messageType === 'emote' ||
		messageType === 'image' ||
		messageType === 'audio'
			? messageType
			: 'text';
	if (
		normalizedMessageType === 'text' ||
		normalizedMessageType === 'notice' ||
		normalizedMessageType === 'emote'
	) {
		if (!isOptionalString(event.formattedBody)) {
			return undefined;
		}
	} else if (event.formattedBody !== undefined) {
		return undefined;
	}
	if (
		!isOptionalString(event.fileName) ||
		!isOptionalString(event.mimeType) ||
		!isOptionalNonNegativeNumber(event.sizeBytes) ||
		!isOptionalNonNegativeNumber(event.width) ||
		!isOptionalNonNegativeNumber(event.height) ||
		!isOptionalNonNegativeNumber(event.durationSeconds)
	) {
		return undefined;
	}

	if (event.mentions !== undefined && !isMentions(event.mentions)) {
		return undefined;
	}
	const mentions = event.mentions;

	const hasRelationType = event.relationType !== undefined;
	const hasTargetEventId = event.targetEventId !== undefined;
	if (hasRelationType !== hasTargetEventId) {
		return undefined;
	}
	if (
		hasRelationType &&
		(!isRelationshipType(event.relationType) || !isValidEventId(event.targetEventId))
	) {
		return undefined;
	}
	const relationType = event.relationType as MatrixE2eeRelationshipType | undefined;
	const targetEventId = event.targetEventId as string | undefined;

	const roomId = event.roomId as string;
	const sender = event.sender as string;
	const eventId = event.eventId as string;
	const body = event.body as string;
	const formattedBody = event.formattedBody as string | undefined;
	const fileName = event.fileName as string | undefined;
	const mimeType = event.mimeType as string | undefined;
	const sizeBytes = event.sizeBytes as number | undefined;
	const width = event.width as number | undefined;
	const height = event.height as number | undefined;
	const durationSeconds = event.durationSeconds as number | undefined;

	return {
		eventType: 'message',
		roomId,
		sender,
		eventId,
		body,
		...(formattedBody === undefined ? {} : { formattedBody }),
		messageType: normalizedMessageType,
		...(mentions === undefined
			? {}
			: { mentions: { userIds: [...mentions.userIds], room: mentions.room } }),
		...(relationType === undefined || targetEventId === undefined
			? {}
			: { relationType, targetEventId }),
		fileName,
		mimeType,
		sizeBytes,
		width,
		height,
		durationSeconds,
	};
}

function shouldEmitEvent(
	event: MatrixE2eeEvent,
	triggerEventMode: MatrixE2eeTriggerEventMode,
	includeEdits: boolean,
	allowedRoomEvents: Set<MatrixE2eeRoomEventType>,
): boolean {
	switch (triggerEventMode) {
		case 'message':
			return event.eventType === 'message' && (includeEdits || event.relationType !== 'edit');
		case 'reaction':
			return event.eventType === 'reaction';
		case 'room':
			return (
				(event.eventType === 'invite' ||
					event.eventType === 'join' ||
					event.eventType === 'leave') &&
				allowedRoomEvents.has(event.eventType)
			);
	}
}

export function isMatrixE2eeEvent(payload: unknown): payload is IDataObject {
	return normalizeMatrixE2eeEvent(payload) !== undefined;
}

function normalizeBinaryResponse(responseData: unknown): Buffer | undefined {
	if (Buffer.isBuffer(responseData)) {
		return responseData;
	}
	if (responseData instanceof ArrayBuffer) {
		return Buffer.from(responseData);
	}
	if (ArrayBuffer.isView(responseData)) {
		return Buffer.from(responseData.buffer, responseData.byteOffset, responseData.byteLength);
	}

	return undefined;
}

async function downloadGatewayMedia(
	this: ITriggerFunctions,
	baseUrl: string,
	apiToken: string,
	event: MatrixE2eeMessageEvent,
): Promise<IBinaryData> {
	// Push-trigger callbacks run outside n8n's credential expression context.
	// The token was resolved and validated during activation, so use it directly.
	const responseData = await this.helpers.httpRequest.call(this, {
		method: 'GET',
		url: getGatewayMediaUrl(baseUrl),
		headers: {
			Authorization: `Bearer ${apiToken}`,
		},
		qs: {
			roomId: event.roomId,
			eventId: event.eventId,
		},
		encoding: 'arraybuffer',
	});
	const buffer = normalizeBinaryResponse(responseData);

	if (buffer === undefined) {
		throw new NodeOperationError(
			this.getNode(),
			'Gateway media download did not return binary data.',
		);
	}

	const fileName = event.fileName?.trim() || 'media';
	const mimeType = event.mimeType?.trim() || undefined;

	return this.helpers.prepareBinaryData(buffer, fileName, mimeType);
}

async function buildExecutionData(
	this: ITriggerFunctions,
	baseUrl: string,
	apiToken: string,
	event: MatrixE2eeEvent,
	downloadMedia: boolean,
	binaryPropertyName: string,
): Promise<INodeExecutionData> {
	const item: INodeExecutionData = {
		json: { ...event },
	};

	if (
		event.eventType === 'message' &&
		(event.messageType === 'image' || event.messageType === 'audio') &&
		downloadMedia
	) {
		item.binary = {
			[binaryPropertyName]: await downloadGatewayMedia.call(this, baseUrl, apiToken, event),
		};
	}

	return item;
}

export class MatrixE2eeTrigger implements INodeType {
	description: INodeTypeDescription = {
		displayName: 'Matrix E2EE Trigger',
		name: 'matrixE2eeTrigger',
		icon: {
			light: 'file:matrix-official.svg',
			dark: 'file:matrix-official.dark.svg',
		},
		group: ['trigger'],
		version: [1],
		subtitle:
			'={{$parameter["event"] === "reaction" ? "On Reaction" : $parameter["event"] === "room" ? "On Room Event" : "On Message"}}',
		description: 'Send and receive encrypted Matrix messages',
		defaults: {
			name: 'Matrix E2EE Trigger',
		},
		inputs: [],
		outputs: [NodeConnectionTypes.Main],
		credentials: [
			{
				name: 'matrixE2eeApi',
				required: true,
			},
		],
		properties: [
			{
				displayName: 'Event',
				name: 'event',
				type: 'options',
				noDataExpression: true,
				options: [
					{
						name: 'On Message',
						value: 'message',
						action: 'On message',
						description: 'Trigger when a new encrypted message is received',
					},
					{
						name: 'On Reaction',
						value: 'reaction',
						action: 'On reaction',
						description: 'Trigger when a reaction is added to a message',
					},
					{
						name: 'On Room Event',
						value: 'room',
						action: 'On room event',
						description:
							'Trigger when a room invitation is received or a user joins or leaves a room',
					},
				],
				default: 'message',
				description: 'The Matrix event category that triggers the workflow',
			},
			{
				displayName: 'Room Events',
				name: 'roomEvents',
				type: 'multiOptions',
				displayOptions: {
					show: {
						event: ['room'],
					},
				},
				options: [
					{
						name: 'Invite',
						value: 'invite',
						description: 'Trigger when a room invitation is received',
					},
					{
						name: 'Join',
						value: 'join',
						description: 'Trigger when a user joins a room',
					},
					{
						name: 'Leave',
						value: 'leave',
						description: 'Trigger when a user leaves or is removed from a room',
					},
				],
				default: ['invite', 'join', 'leave'],
				description: 'Which room membership events should trigger the workflow',
			},
			{
				displayName: 'Include Edits',
				name: 'includeEdits',
				type: 'boolean',
				default: false,
				displayOptions: {
					show: {
						event: ['message'],
					},
				},
				description: 'Whether to also trigger when an existing message is edited',
			},
			{
				displayName: 'Download Media',
				name: 'downloadMedia',
				type: 'boolean',
				default: true,
				displayOptions: {
					show: {
						event: ['message'],
					},
				},
				description:
					'Whether to download image or audio message content and attach it as binary data to the output item',
			},
			{
				displayName: 'Output Data Field Name',
				name: 'binaryPropertyName',
				type: 'string',
				default: 'data',
				displayOptions: {
					show: {
						event: ['message'],
						downloadMedia: [true],
					},
				},
				description: 'The output binary property name to use for downloaded image or audio content',
			},
		],
	};

	async trigger(this: ITriggerFunctions): Promise<ITriggerResponse> {
		const credentials = await this.getCredentials('matrixE2eeApi');
		const baseUrl = credentials.baseUrl;
		const apiToken = credentials.apiToken;
		const rawEventMode = this.getNodeParameter('event', 'message') as unknown;
		const triggerEventMode: MatrixE2eeTriggerEventMode =
			rawEventMode === 'reaction' || rawEventMode === 'room' ? rawEventMode : 'message';
		const includeEdits =
			triggerEventMode === 'message'
				? Boolean(this.getNodeParameter('includeEdits', false))
				: false;
		const rawRoomEvents =
			triggerEventMode === 'room'
				? (this.getNodeParameter('roomEvents', ['invite', 'join', 'leave']) as unknown)
				: ['invite', 'join', 'leave'];
		const allowedRoomEvents = new Set<MatrixE2eeRoomEventType>(
			Array.isArray(rawRoomEvents)
				? rawRoomEvents.filter(
						(entry): entry is MatrixE2eeRoomEventType =>
							entry === 'invite' || entry === 'join' || entry === 'leave',
					)
				: ['invite', 'join', 'leave'],
		);
		const downloadMedia =
			triggerEventMode === 'message'
				? Boolean(this.getNodeParameter('downloadMedia', true))
				: false;
		const binaryPropertyName =
			triggerEventMode === 'message'
				? String(this.getNodeParameter('binaryPropertyName', 'data'))
				: 'data';

		if (typeof baseUrl !== 'string' || baseUrl.trim().length === 0) {
			throw new NodeOperationError(
				this.getNode(),
				'The Gateway Base URL credential must be a non-empty string.',
			);
		}
		if (typeof apiToken !== 'string' || apiToken.trim().length === 0) {
			throw new NodeOperationError(
				this.getNode(),
				'The API Token credential must be a non-empty string.',
			);
		}
		if (downloadMedia && binaryPropertyName.trim().length === 0) {
			throw new NodeOperationError(this.getNode(), 'Output Data Field Name must not be empty.');
		}

		let wsUrl: string;
		try {
			wsUrl = getGatewayWebSocketUrl(baseUrl);
		} catch {
			throw new NodeOperationError(
				this.getNode(),
				'The Gateway Base URL credential must be a valid HTTP or HTTPS URL.',
			);
		}
		if (wsUrl.length === 0) {
			throw new NodeOperationError(
				this.getNode(),
				'The Gateway Base URL credential must use the http or https protocol.',
			);
		}

		if (typeof WebSocket === 'undefined') {
			throw new NodeOperationError(
				this.getNode(),
				'Node.js 22.4 or later with WebSocket support is required.',
			);
		}

		let active = true;
		let socket: WebSocket | undefined;
		let reconnectTimer: AbortController | undefined;
		let reconnectAttempts = 0;
		let authTimer: AbortController | undefined;
		const clearAuthTimer = (): void => {
			authTimer?.abort();
			authTimer = undefined;
		};

		const clearReconnectTimer = (): void => {
			if (reconnectTimer !== undefined) {
				reconnectTimer.abort();
				reconnectTimer = undefined;
			}
		};

		const scheduleReconnect = (): void => {
			if (!active || socket !== undefined || reconnectTimer !== undefined) {
				return;
			}

			const delay = Math.min(
				INITIAL_RECONNECT_DELAY_MS * 2 ** reconnectAttempts,
				MAX_RECONNECT_DELAY_MS,
			);
			reconnectAttempts += 1;

			const timer = new AbortController();
			reconnectTimer = timer;
			void sleep(delay, timer.signal)
				.then(() => {
					if (timer.signal.aborted) return;
					reconnectTimer = undefined;
					connect();
				})
				.catch(() => {
					/* Cancellation during workflow shutdown. */
				});
		};

		const connect = (): void => {
			if (!active || socket !== undefined) {
				return;
			}

			let nextSocket: WebSocket;
			try {
				nextSocket = new WebSocket(wsUrl);
			} catch {
				scheduleReconnect();
				return;
			}

			socket = nextSocket;

			let authenticated = false;
			const deadline = new AbortController();
			authTimer = deadline;
			void sleep(AUTH_TIMEOUT_MS, deadline.signal)
				.then(() => {
					if (!deadline.signal.aborted && socket === nextSocket) nextSocket.close();
				})
				.catch(() => {
					/* Authentication completed or workflow stopped. */
				});
			nextSocket.addEventListener('open', () => {
				if (active && socket === nextSocket) {
					nextSocket.send(JSON.stringify({ type: 'authenticate', token: apiToken }));
				}
			});

			nextSocket.addEventListener('message', ({ data }) => {
				if (!active || socket !== nextSocket) {
					return;
				}

				void (async () => {
					if (typeof data !== 'string') return;
					const payload = JSON.parse(data) as unknown;
					if (!authenticated) {
						if (
							typeof payload === 'object' &&
							payload !== null &&
							'type' in payload &&
							payload.type === 'authenticated'
						) {
							authenticated = true;
							reconnectAttempts = 0;
							clearAuthTimer();
						} else {
							nextSocket.close();
						}
						return;
					}
					const event = normalizeMatrixE2eeEvent(payload);

					if (
						event === undefined ||
						!shouldEmitEvent(event, triggerEventMode, includeEdits, allowedRoomEvents)
					) {
						return;
					}

					const executionData = await buildExecutionData.call(
						this,
						baseUrl,
						apiToken,
						event,
						downloadMedia,
						binaryPropertyName,
					);
					if (active && socket === nextSocket) this.emit([[executionData]]);
				})().catch((error: unknown) => {
					const message = error instanceof Error ? error.message : 'Unknown error';
					this.saveFailedExecution(new NodeOperationError(this.getNode(), message));
				});
			});

			nextSocket.addEventListener('error', () => {
				// The close handler performs reconnection after transport errors.
			});

			nextSocket.addEventListener('close', (event) => {
				if (socket === nextSocket) {
					socket = undefined;
					clearAuthTimer();
					if (event.code === 4401) {
						active = false;
						this.saveFailedExecution(
							new NodeOperationError(
								this.getNode(),
								'Gateway authentication failed. Check the API Token and reactivate the workflow.',
							),
						);
					} else scheduleReconnect();
				}
			});
		};

		connect();

		return {
			closeFunction: async () => {
				active = false;
				clearReconnectTimer();
				clearAuthTimer();

				const currentSocket = socket;
				socket = undefined;

				if (
					currentSocket !== undefined &&
					(currentSocket.readyState === WebSocket.CONNECTING ||
						currentSocket.readyState === WebSocket.OPEN)
				) {
					currentSocket.close();
				}
			},
		};
	}
}
