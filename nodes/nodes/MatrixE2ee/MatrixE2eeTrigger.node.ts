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

interface MatrixE2eeMentions extends IDataObject {
	user_ids: string[];
	room: boolean;
}

interface MatrixE2eeRelatesTo extends IDataObject {
	rel_type: MatrixE2eeRelationshipType;
	event_id: string;
}

interface MatrixE2eeReactionRelatesTo extends IDataObject {
	rel_type: 'annotation';
	event_id: string;
	key: string;
}

interface MatrixE2eeMessageEvent extends IDataObject {
	event_type: 'room.message';
	room_id: string;
	sender: string;
	event_id: string;
	body: string;
	formatted_body?: string;
	message_type: MatrixE2eeMessageType;
	mentions?: MatrixE2eeMentions;
	relates_to?: MatrixE2eeRelatesTo;
	file_name?: string;
	mime_type?: string;
	size_bytes?: number;
	width?: number;
	height?: number;
	duration_seconds?: number;
}

interface MatrixE2eeReactionEvent extends IDataObject {
	event_type: 'reaction';
	room_id: string;
	sender: string;
	event_id: string;
	key: string;
	relates_to: MatrixE2eeReactionRelatesTo;
}

interface MatrixE2eeInviteEvent extends IDataObject {
	event_type: 'room.invite';
	room_id: string;
	sender: string;
}

type MatrixE2eeEvent =
	| MatrixE2eeMessageEvent
	| MatrixE2eeReactionEvent
	| MatrixE2eeInviteEvent;

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
		Array.isArray(mentions.user_ids) &&
		mentions.user_ids.every(isNonEmptyString) &&
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

function isRelatesTo(value: unknown): value is MatrixE2eeRelatesTo {
	if (typeof value !== 'object' || value === null || Array.isArray(value)) {
		return false;
	}
	const relatesTo = value as Record<string, unknown>;
	return (
		(relatesTo.rel_type === 'thread' ||
			relatesTo.rel_type === 'reply' ||
			relatesTo.rel_type === 'edit') &&
		isValidEventId(relatesTo.event_id)
	);
}

function isReactionRelatesTo(value: unknown): value is MatrixE2eeReactionRelatesTo {
	if (typeof value !== 'object' || value === null || Array.isArray(value)) {
		return false;
	}
	const relatesTo = value as Record<string, unknown>;
	return (
		relatesTo.rel_type === 'annotation' &&
		isValidEventId(relatesTo.event_id) &&
		isNonEmptyString(relatesTo.key)
	);
}

function normalizeMatrixE2eeEvent(payload: unknown): MatrixE2eeEvent | undefined {
	if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
		return undefined;
	}

	const event = payload as Record<string, unknown>;

	if (event.event_type === 'room.invite') {
		if (!['room_id', 'sender'].every((field) => isNonEmptyString(event[field]))) {
			return undefined;
		}

		return {
			event_type: 'room.invite',
			room_id: event.room_id as string,
			sender: event.sender as string,
		};
	}

	if (event.event_type === 'reaction') {
		if (
			!['room_id', 'sender', 'event_id', 'key'].every((field) => isNonEmptyString(event[field])) ||
			!isReactionRelatesTo(event.relates_to) ||
			event.relates_to.key !== event.key
		) {
			return undefined;
		}

		return {
			event_type: 'reaction',
			room_id: event.room_id as string,
			sender: event.sender as string,
			event_id: event.event_id as string,
			key: event.key as string,
			relates_to: {
				rel_type: 'annotation',
				event_id: event.relates_to.event_id,
				key: event.relates_to.key,
			},
		};
	}

	if (
		event.event_type !== 'room.message' ||
		!['room_id', 'sender', 'event_id', 'body'].every((field) => isNonEmptyString(event[field]))
	) {
		return undefined;
	}

	const messageType = event.message_type;
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
		if (!isOptionalString(event.formatted_body)) {
			return undefined;
		}
	} else if (event.formatted_body !== undefined) {
		return undefined;
	}
	if (
		!isOptionalString(event.file_name) ||
		!isOptionalString(event.mime_type) ||
		!isOptionalNonNegativeNumber(event.size_bytes) ||
		!isOptionalNonNegativeNumber(event.width) ||
		!isOptionalNonNegativeNumber(event.height) ||
		!isOptionalNonNegativeNumber(event.duration_seconds)
	) {
		return undefined;
	}

	if (event.mentions !== undefined && !isMentions(event.mentions)) {
		return undefined;
	}
	const mentions = event.mentions;

	if (event.relates_to !== undefined && !isRelatesTo(event.relates_to)) {
		return undefined;
	}
	const relatesTo = event.relates_to;

	const roomId = event.room_id as string;
	const sender = event.sender as string;
	const eventId = event.event_id as string;
	const body = event.body as string;
	const formattedBody = event.formatted_body as string | undefined;
	const fileName = event.file_name as string | undefined;
	const mimeType = event.mime_type as string | undefined;
	const sizeBytes = event.size_bytes as number | undefined;
	const width = event.width as number | undefined;
	const height = event.height as number | undefined;
	const durationSeconds = event.duration_seconds as number | undefined;

	return {
		event_type: 'room.message',
		room_id: roomId,
		sender,
		event_id: eventId,
		body,
		...(formattedBody === undefined ? {} : { formatted_body: formattedBody }),
		message_type: normalizedMessageType,
		...(mentions === undefined
			? {}
			: { mentions: { user_ids: [...mentions.user_ids], room: mentions.room } }),
		...(relatesTo === undefined
			? {}
			: { relates_to: { rel_type: relatesTo.rel_type, event_id: relatesTo.event_id } }),
		file_name: fileName,
		mime_type: mimeType,
		size_bytes: sizeBytes,
		width,
		height,
		duration_seconds: durationSeconds,
	};
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
			room_id: event.room_id,
			event_id: event.event_id,
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

	const fileName = event.file_name?.trim() || 'media';
	const mimeType = event.mime_type?.trim() || undefined;

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
		event.event_type === 'room.message' &&
		(event.message_type === 'image' || event.message_type === 'audio') &&
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
		subtitle: 'New message',
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
				displayName: 'Download Media',
				name: 'downloadMedia',
				type: 'boolean',
				default: true,
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
		const downloadMedia = this.getNodeParameter('downloadMedia') as boolean;
		const binaryPropertyName = this.getNodeParameter('binaryPropertyName') as string;

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

					if (event === undefined) {
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
