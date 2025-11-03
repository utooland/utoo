import assert from 'assert';
import buffer from 'buffer';
const stream = require('stream');
import { process } from 'browser-polyfill';
import { urlToHttpOptions } from 'url';
import timers from 'timers';

urlToHttpOptions;
timers;

const fs = require('fs');

fs;

console.log(assert, buffer, process);

console.log(stream);