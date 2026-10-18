import type {
	IBinaryData,
	IDataObject,
	IExecuteFunctions,
	INodeExecutionData,
	INodeType,
	INodeTypeDescription,
} from 'n8n-workflow';
import { NodeApiError, NodeConnectionTypes, NodeOperationError } from 'n8n-workflow';

export function getGatewayApiUrl(baseUrl: string, path: string): string {
	return `${baseUrl.trim().replace(/\/+$/, '')}${path}`;
}

export function getGatewaySendTextUrl(baseUrl: string): string {
	return getGatewayApiUrl(baseUrl, '/v1/messages/text');
}

export function getGatewaySendImageUrl(baseUrl: string): string {
	return getGatewayApiUrl(baseUrl, '/v1/messages/image');
}

export function getGatewaySendAudioUrl(baseUrl: string): string {
	return getGatewayApiUrl(baseUrl, '/v1/messages/audio');
}

export function getGatewayTypingUrl(baseUrl: string): string {
	return getGatewayApiUrl(baseUrl, '/v1/typing');
}

function trimmedOrUndefined(value: string): string | undefined {
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : undefined;
}

function imageExtensionFromMimeType(mimeType: string): string | undefined {
	const [, subtype] = mimeType.split('/', 2);
	return subtype?.split('+', 2)[0];
}

function resolveImageMimeType(binaryData: IBinaryData): string | undefined {
	const mimeType = trimmedOrUndefined(binaryData.mimeType ?? '');

	return mimeType?.startsWith('image/') ? mimeType : undefined;
}

function resolveAudioMimeType(binaryData: IBinaryData): string | undefined {
	const mimeType = trimmedOrUndefined(binaryData.mimeType ?? '');
	return mimeType === 'audio/ogg' ? mimeType : undefined;
}

function resolveImageFileName(binaryData: IBinaryData, mimeType: string): string {
	const fileName = trimmedOrUndefined(binaryData.fileName ?? '');

	if (fileName !== undefined) {
		return fileName;
	}

	const extension = imageExtensionFromMimeType(mimeType);
	return extension === undefined ? 'image' : `image.${extension}`;
}

export class MatrixE2ee implements INodeType {
	description: INodeTypeDescription = {
		displayName: 'Matrix E2EE',
		name: 'matrixE2ee',
		icon: {
			light: 'file:matrix-official.svg',
			dark: 'file:matrix-official.dark.svg',
		},
		group: ['output'],
		version: [1],
		subtitle:
			'={{$parameter["operation"] === "typingIndicator" ? "Set typing indicator" : $parameter["messageType"] === "image" ? "Send image" : $parameter["messageType"] === "audio" ? "Send audio" : "Send message"}}',
		description: 'Send and receive encrypted Matrix messages',
		defaults: {
			name: 'Matrix E2EE',
		},
		usableAsTool: true,
		inputs: [NodeConnectionTypes.Main],
		outputs: [NodeConnectionTypes.Main],
		credentials: [
			{
				name: 'matrixE2eeApi',
				required: true,
			},
		],
		properties: [
			{
				displayName: 'Operation',
				name: 'operation',
				type: 'options',
				noDataExpression: true,
				options: [
					{
						name: 'Send Message',
						value: 'message',
						action: 'Send a message',
						description: 'Send text, image, or audio',
					},
					{
						name: 'Typing Indicator',
						value: 'typingIndicator',
						action: 'Show or hide typing status in a room',
						description: 'Show or hide typing status in a room',
					},
				],
				default: 'message',
			},
			{
				displayName: 'Room',
				name: 'roomId',
				type: 'string',
				required: true,
				default: '',
				placeholder: '!room:example.org',
				description: 'The Matrix room ID that will receive the message or typing indicator',
			},
			{
				displayName: 'Message Type',
				name: 'messageType',
				type: 'options',
				noDataExpression: true,
				displayOptions: {
					show: {
						operation: ['message'],
					},
				},
				options: [
					{
						name: 'Text',
						value: 'text',
						description: 'Send a text message with full Markdown support',
					},
					{
						name: 'Image',
						value: 'image',
						description: 'Send an image from incoming binary data',
					},
					{
						name: 'Audio',
						value: 'audio',
						description: 'Send Ogg/Opus audio from incoming binary data',
					},
				],
				default: 'text',
				description: 'The type of Matrix message to send',
			},
			{
				displayName: 'Message',
				name: 'message',
				type: 'string',
				required: true,
				default: '',
				typeOptions: {
					rows: 4,
				},
				displayOptions: {
					show: {
						operation: ['message'],
						messageType: ['text'],
					},
				},
				description: 'The message to send (supports Markdown)',
			},
			{
				displayName: 'Input Data Field Name',
				name: 'binaryPropertyName',
				type: 'string',
				required: true,
				default: 'data',
				typeOptions: {
					binaryDataProperty: true,
				},
				displayOptions: {
					show: {
						operation: ['message'],
						messageType: ['image', 'audio'],
					},
				},
				description: 'The name of the input binary property that contains the image or audio data',
			},
			{
				displayName: 'Caption',
				name: 'caption',
				type: 'string',
				default: '',
				typeOptions: {
					rows: 2,
				},
				displayOptions: {
					show: {
						operation: ['message'],
						messageType: ['image', 'audio'],
					},
				},
				description: 'Optional caption to send with the image or audio (maps to Matrix body)',
			},
			{
				displayName: 'Typing State',
				name: 'typingState',
				type: 'options',
				required: true,
				default: 'start',
				displayOptions: {
					show: {
						operation: ['typingIndicator'],
					},
				},
				options: [
					{
						name: 'Start Typing',
						value: 'start',
						action: 'Start typing in the room',
						description: 'Show the typing indicator in the room',
					},
					{
						name: 'Stop Typing',
						value: 'stop',
						action: 'Stop typing in the room',
						description: 'Hide the typing indicator in the room',
					},
				],
				description: 'Whether the gateway bot should start or stop typing in the room',
			},
			{
				displayName: 'Timeout / Duration',
				name: 'timeoutSeconds',
				type: 'number',
				required: true,
				default: 30,
				typeOptions: {
					minValue: 1,
				},
				displayOptions: {
					show: {
						operation: ['typingIndicator'],
						typingState: ['start'],
					},
				},
				description: 'How long the typing indicator should remain active in seconds',
			},
		],
	};

	async execute(this: IExecuteFunctions): Promise<INodeExecutionData[][]> {
		const items = this.getInputData();
		const returnData: INodeExecutionData[] = [];
		const credentials = await this.getCredentials('matrixE2eeApi');
		const baseUrl = credentials.baseUrl;

		if (typeof baseUrl !== 'string' || baseUrl.trim().length === 0) {
			throw new NodeOperationError(
				this.getNode(),
				'The Gateway Base URL credential must be a non-empty string.',
			);
		}

		for (let itemIndex = 0; itemIndex < items.length; itemIndex++) {
			try {
				const operation = this.getNodeParameter('operation', itemIndex) as string;
				const roomId = this.getNodeParameter('roomId', itemIndex) as string;

				if (roomId.trim().length === 0) {
					throw new NodeOperationError(this.getNode(), 'Room must not be empty.', { itemIndex });
				}
				let responseData: unknown;

				switch (operation) {
					case 'message': {
						const messageType = this.getNodeParameter('messageType', itemIndex) as string;

						switch (messageType) {
							case 'text': {
								const message = this.getNodeParameter('message', itemIndex) as string;

								if (message.trim().length === 0) {
									throw new NodeOperationError(this.getNode(), 'Message must not be empty.', {
										itemIndex,
									});
								}

								responseData = await this.helpers.httpRequestWithAuthentication.call(
									this,
									'matrixE2eeApi',
									{
										method: 'POST',
										url: getGatewaySendTextUrl(baseUrl),
										body: {
											room_id: roomId,
											body: message,
										} satisfies IDataObject,
										json: true,
									},
								);
								break;
							}
							case 'image': {
								const binaryPropertyName = this.getNodeParameter(
									'binaryPropertyName',
									itemIndex,
								) as string;
								const binaryData = this.helpers.assertBinaryData(itemIndex, binaryPropertyName);
								const binaryBuffer = await this.helpers.getBinaryDataBuffer(itemIndex, binaryData);
								const caption = this.getNodeParameter('caption', itemIndex) as string;
								const mimeType = resolveImageMimeType(binaryData);

								if (mimeType === undefined) {
									throw new NodeOperationError(
										this.getNode(),
										'Image MIME type is required in binary metadata and must start with image/.',
										{ itemIndex },
									);
								}

								const fileName = resolveImageFileName(binaryData, mimeType);
								const qs: IDataObject = {
									room_id: roomId,
									file_name: fileName,
								};
								const trimmedCaption = trimmedOrUndefined(caption);

								if (trimmedCaption !== undefined) {
									qs.body = trimmedCaption;
								}

								responseData = await this.helpers.httpRequestWithAuthentication.call(
									this,
									'matrixE2eeApi',
									{
										method: 'POST',
										url: getGatewaySendImageUrl(baseUrl),
										qs,
										headers: {
											'Content-Type': mimeType,
										},
										body: binaryBuffer,
										json: true,
									},
								);
								break;
							}
							case 'audio': {
								const binaryPropertyName = this.getNodeParameter(
									'binaryPropertyName',
									itemIndex,
								) as string;
								const binaryData = this.helpers.assertBinaryData(itemIndex, binaryPropertyName);
								const binaryBuffer = await this.helpers.getBinaryDataBuffer(itemIndex, binaryData);
								const caption = this.getNodeParameter('caption', itemIndex) as string;
								const mimeType = resolveAudioMimeType(binaryData);

								if (mimeType === undefined) {
									throw new NodeOperationError(
										this.getNode(),
										'Audio MIME type is required in binary metadata and must be audio/ogg for Ogg/Opus audio.',
										{ itemIndex },
									);
								}

								const fileName =
									trimmedOrUndefined(binaryData.fileName ?? '') ?? 'audio-message.ogg';
								const qs: IDataObject = {
									room_id: roomId,
									file_name: fileName,
								};
								const trimmedCaption = trimmedOrUndefined(caption);

								if (trimmedCaption !== undefined) {
									qs.body = trimmedCaption;
								}
								responseData = await this.helpers.httpRequestWithAuthentication.call(
									this,
									'matrixE2eeApi',
									{
										method: 'POST',
										url: getGatewaySendAudioUrl(baseUrl),
										qs,
										headers: {
											'Content-Type': mimeType,
										},
										body: binaryBuffer,
										json: true,
									},
								);
								break;
							}
							default:
								throw new NodeOperationError(
									this.getNode(),
									`Unsupported message type: ${messageType}`,
									{ itemIndex },
								);
						}
						break;
					}
					case 'typingIndicator': {
						const typingState = this.getNodeParameter('typingState', itemIndex) as string;
						const typing = typingState === 'start';
						const requestBody: IDataObject = {
							room_id: roomId,
							typing,
						};

						if (typing) {
							const timeoutSeconds = this.getNodeParameter('timeoutSeconds', itemIndex) as number;
							if (
								typeof timeoutSeconds !== 'number' ||
								!Number.isFinite(timeoutSeconds) ||
								timeoutSeconds <= 0
							) {
								throw new NodeOperationError(
									this.getNode(),
									'Timeout / Duration must be a number greater than 0.',
									{ itemIndex },
								);
							}
							requestBody.timeout_seconds = Math.round(timeoutSeconds);
						}

						responseData = await this.helpers.httpRequestWithAuthentication.call(
							this,
							'matrixE2eeApi',
							{
								method: 'POST',
								url: getGatewayTypingUrl(baseUrl),
								body: requestBody,
								json: true,
							},
						);
						break;
					}
					default:
						throw new NodeOperationError(this.getNode(), `Unsupported operation: ${operation}`, {
							itemIndex,
						});
				}

				returnData.push({
					json: responseData as IDataObject,
					pairedItem: { item: itemIndex },
				});
			} catch (error) {
				if (this.continueOnFail()) {
					returnData.push({
						json: {
							error: error instanceof Error ? error.message : 'Unknown error',
						},
						pairedItem: { item: itemIndex },
					});
					continue;
				}

				if (error instanceof NodeOperationError) {
					throw new NodeOperationError(this.getNode(), error.message, { itemIndex });
				}

				throw new NodeApiError(
					this.getNode(),
					{ message: error instanceof Error ? error.message : 'Unknown error' },
					{ itemIndex },
				);
			}
		}

		return [returnData];
	}
}
