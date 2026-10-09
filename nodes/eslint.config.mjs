import { configWithoutCloudSupport } from '@n8n/node-cli/eslint';

export default [
	...configWithoutCloudSupport,
	{
		rules: {
			'@n8n/community-nodes/no-runtime-dependencies': 'off',
		},
	},
];
