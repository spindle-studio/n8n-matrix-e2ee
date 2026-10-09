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

function resolveImageMimeType(binaryData: IBinaryData, mimeTypeParameter: string): string | undefined {
	const mimeType = trimmedOrUndefined(mimeTypeParameter) ?? trimmedOrUndefined(binaryData.mimeType ?? '');

	return mimeType?.startsWith('image/') ? mimeType : undefined;
}

function resolveAudioMimeType(binaryData: IBinaryData, mimeTypeParameter: string): string | undefined {
	const mimeType = trimmedOrUndefined(mimeTypeParameter) ?? trimmedOrUndefined(binaryData.mimeType ?? '');
	return mimeType === 'audio/ogg' ? mimeType : undefined;
}

function resolveImageFileName(
	binaryData: IBinaryData,
	fileNameParameter: string,
	mimeType: string,
): string {
	const fileName =
		trimmedOrUndefined(fileNameParameter) ?? trimmedOrUndefined(binaryData.fileName ?? '');

	if (fileName !== undefined) {
		return fileName;
	}

	const extension = imageExtensionFromMimeType(mimeType);
	return extension === undefined ? 'image' : `image.${extension}`;
}

export class MatrixE2eeAction implements INodeType {
	description: INodeTypeDescription = {
		displayName: 'Matrix E2EE',
		name: 'matrixE2eeAction',
		icon: {
			light: 'file:matrix-official.svg',
			dark: 'file:matrix-official.dark.svg',
		},
		group: ['output'],
		version: [1],
		subtitle:
			'={{$parameter["operation"] === "setTyping" ? "Set typing indicator" : $parameter["operation"] === "sendImage" ? "Send image" : $parameter["operation"] === "sendAudio" ? "Send audio" : "Send message"}}',
		description: 'Send Matrix messages, images, audio, and typing indicators through the Matrix E2EE gateway',
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
						value: 'sendMessage',
						action: 'Send a message',
						description: 'Send a text message to a Matrix room',
					},
					{
						name: 'Send Image',
						value: 'sendImage',
						action: 'Send an image',
						description: 'Send an image to a Matrix room from incoming binary data',
					},
					{
						name: 'Send Audio',
						value: 'sendAudio',
						action: 'Send audio',
						description: 'Send Ogg/Opus audio from incoming binary data',
					},
					{
						name: 'Set Typing Indicator',
						value: 'setTyping',
						action: 'Show or hide a typing indicator',
						description: 'Show or hide the gateway bot typing indicator in a Matrix room',
					},
				],
				default: 'sendMessage',
			},
			{
				displayName: 'Room ID',
				name: 'roomId',
				type: 'string',
				required: true,
				default: '',
				placeholder: '!room:example.org',
				description: 'The Matrix room ID that will receive the message',
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
						operation: ['sendMessage'],
					},
				},
				description: 'The message to send',
			},
			{
				displayName: 'Binary Property',
				name: 'binaryPropertyName',
				type: 'string',
				required: true,
				default: 'image',
				typeOptions: {
					binaryDataProperty: true,
				},
				displayOptions: {
					show: {
						operation: ['sendImage', 'sendAudio'],
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
						operation: ['sendImage', 'sendAudio'],
					},
				},
				description: 'Optional caption text to send with the image',
			},
			{
				displayName: 'File Name',
				name: 'fileName',
				type: 'string',
				default: '',
				displayOptions: {
					show: {
						operation: ['sendImage', 'sendAudio'],
					},
				},
				description: 'Optional file name to use instead of the binary metadata file name',
			},
			{
				displayName: 'MIME Type',
				name: 'mimeType',
				type: 'string',
				default: '',
				placeholder: 'image/png or audio/ogg',
				displayOptions: {
					show: {
						operation: ['sendImage', 'sendAudio'],
					},
				},
				description: 'Optional image or Ogg/Opus audio MIME type to use instead of binary metadata',
			},
			{
				displayName: 'Typing State',
				name: 'typingState',
				type: 'options',
				required: true,
				default: 'show',
				displayOptions: {
					show: {
						operation: ['setTyping'],
					},
				},
				options: [
					{
						name: 'Show',
						value: 'show',
						action: 'Show the typing indicator',
						description: 'Show the gateway bot typing indicator in the room',
					},
					{
						name: 'Hide',
						value: 'hide',
						action: 'Hide the typing indicator',
						description: 'Hide the gateway bot typing indicator in the room',
					},
				],
				description: 'Whether the gateway bot should appear as typing in the room',
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
					throw new NodeOperationError(this.getNode(), 'Room ID must not be empty.', { itemIndex });
				}
				let responseData: unknown;

				switch (operation) {
					case 'sendMessage': {
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
									message,
								} satisfies IDataObject,
								json: true,
							},
						);
						break;
					}
					case 'sendImage': {
						const binaryPropertyName = this.getNodeParameter(
							'binaryPropertyName',
							itemIndex,
						) as string;
						const binaryData = this.helpers.assertBinaryData(itemIndex, binaryPropertyName);
						const binaryBuffer = await this.helpers.getBinaryDataBuffer(itemIndex, binaryData);
						const caption = this.getNodeParameter('caption', itemIndex) as string;
						const fileNameParameter = this.getNodeParameter('fileName', itemIndex) as string;
						const mimeTypeParameter = this.getNodeParameter('mimeType', itemIndex) as string;
						const mimeType = resolveImageMimeType(binaryData, mimeTypeParameter);

						if (mimeType === undefined) {
							throw new NodeOperationError(
								this.getNode(),
								'Image MIME type is required and must start with image/.',
								{ itemIndex },
							);
						}

						const fileName = resolveImageFileName(binaryData, fileNameParameter, mimeType);
						const qs: IDataObject = {
							room_id: roomId,
							file_name: fileName,
						};
						const trimmedCaption = trimmedOrUndefined(caption);

						if (trimmedCaption !== undefined) {
							qs.caption = trimmedCaption;
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
					case 'sendAudio': {
						const binaryPropertyName = this.getNodeParameter(
							'binaryPropertyName',
							itemIndex,
						) as string;
						const binaryData = this.helpers.assertBinaryData(itemIndex, binaryPropertyName);
						const binaryBuffer = await this.helpers.getBinaryDataBuffer(itemIndex, binaryData);
						const caption = this.getNodeParameter('caption', itemIndex) as string;
						const fileNameParameter = this.getNodeParameter('fileName', itemIndex) as string;
						const mimeTypeParameter = this.getNodeParameter('mimeType', itemIndex) as string;
						const mimeType = resolveAudioMimeType(binaryData, mimeTypeParameter);

						if (mimeType === undefined) {
							throw new NodeOperationError(
								this.getNode(),
								'Audio MIME type is required and must be audio/ogg for Ogg/Opus audio.',
								{ itemIndex },
							);
						}

						const fileName =
							trimmedOrUndefined(fileNameParameter) ??
							trimmedOrUndefined(binaryData.fileName ?? '') ??
							'audio-message.ogg';
						const qs: IDataObject = {
							room_id: roomId,
							file_name: fileName,
						};
						const trimmedCaption = trimmedOrUndefined(caption);

						if (trimmedCaption !== undefined) {
							qs.caption = trimmedCaption;
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
					case 'setTyping': {
						const typingState = this.getNodeParameter('typingState', itemIndex) as string;

						responseData = await this.helpers.httpRequestWithAuthentication.call(
							this,
							'matrixE2eeApi',
							{
								method: 'POST',
								url: getGatewayTypingUrl(baseUrl),
								body: {
									room_id: roomId,
									typing: typingState === 'show',
								} satisfies IDataObject,
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
