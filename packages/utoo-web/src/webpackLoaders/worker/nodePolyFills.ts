const assert = require('assert');
const buffer = require('buffer');
const constants = require('constants');
const path = require('path');
const url = require('url');
const util = require('util');
const less = require("less/lib/less-node/index.js").default;
const lessLoader = require("../loaders/lessLoader");

export default {
	assert,
	buffer,
	constants,
	fs: {
		readFile(path: string, options: any, cb: Function) {
			// @ts-ignore
			return self.workerData.readFile(path, options).then((data: any) => cb(null, data),
				(err: Error) => cb(err))
		},
	},
	path,
	url,
	util,
	less,
	["less-loader"]: lessLoader
};
