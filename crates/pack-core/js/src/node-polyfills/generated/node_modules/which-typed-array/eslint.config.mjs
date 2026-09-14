import ljharbConfig from '@ljharb/eslint-config/flat';

export default [
	...ljharbConfig,
	{
		rules: {
			'func-style': ['error', 'declaration'],
			'max-statements-per-line': ['error', { max: 2 }],
			'no-extra-parens': 'off',
		},
	},
];
