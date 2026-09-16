import assert from "assert";
import buffer from "buffer";
const stream = require("stream");
import { process } from "browser-polyfill";
import { urlToHttpOptions } from "url";
import timers from "timers";
import "./empty";
import { Buffer as NodeBuffer } from "node:buffer";

const nodeFs = require("node:fs");

confirm;

urlToHttpOptions;
timers;

const fs = require("fs");

fs;

console.log(assert, buffer, process);

console.log(stream);

console.log(NodeBuffer, nodeFs);
