'use strict';

var forEach = require('for-each');
var availableTypedArrays = require('available-typed-arrays');
var callBind = require('call-bind');
var callBound = require('call-bound');
var gOPD = require('gopd');
var getProto = require('get-proto');

var $toString = callBound('Object.prototype.toString');
var hasToStringTag = require('has-tostringtag/shams')();

var g = typeof globalThis === 'undefined' ? global : globalThis;
var typedArrays = availableTypedArrays();

var $slice = callBound('String.prototype.slice');

/** @import { BoundSet, BoundSlice, Cache, Getter } from './types' */
/** @import { TypedArrayName } from '.' */

/** @type {<T = unknown>(array: readonly T[], value: unknown) => number} */
var $indexOf = callBound('Array.prototype.indexOf', true) || function indexOf(array, value) {
	for (var i = 0; i < array.length; i += 1) {
		if (array[i] === value) {
			return i;
		}
	}
	return -1;
};

/** @type {Cache} */
var cache = { __proto__: null };
if (hasToStringTag && gOPD && getProto) {
	forEach(typedArrays, function (typedArray) {
		var arr = new g[typedArray]();
		if (Symbol.toStringTag in arr && getProto) {
			var proto = getProto(arr);
			// @ts-expect-error TS won't narrow inside a closure
			var descriptor = gOPD(proto, Symbol.toStringTag);
			if (!descriptor && proto) {
				var superProto = getProto(proto);
				// @ts-expect-error TS won't narrow inside a closure
				descriptor = gOPD(superProto, Symbol.toStringTag);
			}
			if (descriptor && descriptor.get) {
				var bound = callBind(descriptor.get);
				cache[
					/** @type {`$${TypedArrayName}`} */
					('$' + typedArray)
				] = bound;
			}
		}
	});
} else {
	forEach(typedArrays, function (typedArray) {
		var arr = new g[typedArray]();
		var fn = arr.slice || arr.set;
		if (fn) {
			var bound = /** @type {BoundSlice | BoundSet} */ (
				// @ts-expect-error TODO FIXME
				callBind(fn)
			);
			cache[
				/** @type {`$${TypedArrayName}`} */
				('$' + typedArray)
			] = bound;
		}
	});
}

/** @type {(value: object) => false | TypedArrayName} */
function tryTypedArrays(value) {
	/** @type {ReturnType<typeof tryTypedArrays>} */ var found = false;
	forEach(
		/** @type {Record<`$${TypedArrayName}`, Getter>} */ (cache),
		/** @param {Getter} getter @param {`$${TypedArrayName}`} typedArray */
		function (getter, typedArray) {
			if (!found) {
				try {
					// @ts-expect-error a throw is fine here
					if ('$' + getter(value) === typedArray) {
						found = /** @type {TypedArrayName} */ ($slice(typedArray, 1));
					}
				} catch (e) { /**/ }
			}
		}
	);
	return found;
}

/** @type {(value: object) => false | TypedArrayName} */
function trySlices(value) {
	/** @type {ReturnType<typeof trySlices>} */ var found = false;
	forEach(
		/** @type {Record<`$${TypedArrayName}`, Getter>} */(cache),
		/** @param {Getter} getter @param {`$${TypedArrayName}`} name */ function (getter, name) {
			if (!found) {
				try {
					// @ts-expect-error a throw is fine here
					getter(value);
					found = /** @type {TypedArrayName} */ ($slice(name, 1));
				} catch (e) { /**/ }
			}
		}
	);
	return found;
}

/** @type {(tag: unknown) => tag is typeof typedArrays[number]} */
function isTATag(tag) {
	return $indexOf(typedArrays, tag) > -1;
}

/**
 * @type {import('.')}
 * @param {unknown} value
 */
module.exports = function whichTypedArray(value) {
	if (!value || typeof value !== 'object') {
		return false;
	}
	if (!hasToStringTag) {
		var tag = $slice($toString(value), 8, -1);
		if (isTATag(tag)) {
			return tag;
		}
		if (tag !== 'Object') {
			return false;
		}
		// node < 0.6 hits here on real Typed Arrays
		return trySlices(value);
	}
	if (!gOPD) { return null; } // unknown engine
	return tryTypedArrays(value);
};
