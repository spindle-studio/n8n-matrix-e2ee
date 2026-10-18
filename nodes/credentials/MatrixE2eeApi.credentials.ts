import type {
	IAuthenticateGeneric,
	ICredentialTestRequest,
	ICredentialType,
	INodeProperties,
} from 'n8n-workflow';

export class MatrixE2eeApi implements ICredentialType {
	name = 'matrixE2eeApi';

	displayName = 'Matrix E2EE Gateway API';

	documentationUrl = 'https://github.com/spindle-studio/n8n-matrix-e2ee#3-connect-n8n';

	icon = {
		light: 'file:../nodes/MatrixE2ee/matrix-official.svg',
		dark: 'file:../nodes/MatrixE2ee/matrix-official.dark.svg',
	} as const;

	properties: INodeProperties[] = [
		{
			displayName: 'Gateway Base URL',
			name: 'baseUrl',
			type: 'string',
			default: 'http://n8n-matrix-e2ee-gateway:8080',
			required: true,
			placeholder: 'http://gateway:8080',
			description: 'The HTTP URL of the Matrix E2EE gateway',
		},
		{
			displayName: 'API Token',
			name: 'apiToken',
			type: 'string',
			typeOptions: {
				password: true,
			},
			default: '',
			required: true,
			description: 'The bearer token used to authenticate with the Matrix E2EE gateway',
		},
	];

	authenticate: IAuthenticateGeneric = {
		type: 'generic',
		properties: {
			headers: {
				Authorization: '={{"Bearer " + $credentials.apiToken}}',
			},
		},
	};

	test: ICredentialTestRequest = {
		request: {
			baseURL: '={{$credentials.baseUrl}}',
			url: '/v1/auth-check',
			method: 'GET',
			headers: {
				Authorization: '={{"Bearer " + $credentials.apiToken}}',
			},
		},
	};
}
