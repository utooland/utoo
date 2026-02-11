// node:util/types module for utoo-runtime

const isDate = (v) => v instanceof Date;
const isRegExp = (v) => v instanceof RegExp;
const isPromise = (v) => v instanceof Promise;
const isArray = Array.isArray;
const isTypedArray = (v) => ArrayBuffer.isView(v) && !(v instanceof DataView);
const isUint8Array = (v) => v instanceof Uint8Array;
const isArrayBuffer = (v) => v instanceof ArrayBuffer;
const isSharedArrayBuffer = (v) => typeof SharedArrayBuffer !== "undefined" && v instanceof SharedArrayBuffer;
const isNativeError = (v) => v instanceof Error;
const isSet = (v) => v instanceof Set;
const isMap = (v) => v instanceof Map;
const isWeakMap = (v) => v instanceof WeakMap;
const isWeakSet = (v) => v instanceof WeakSet;
const isDataView = (v) => v instanceof DataView;
const isInt8Array = (v) => v instanceof Int8Array;
const isUint8ClampedArray = (v) => v instanceof Uint8ClampedArray;
const isInt16Array = (v) => v instanceof Int16Array;
const isUint16Array = (v) => v instanceof Uint16Array;
const isInt32Array = (v) => v instanceof Int32Array;
const isUint32Array = (v) => v instanceof Uint32Array;
const isFloat32Array = (v) => v instanceof Float32Array;
const isFloat64Array = (v) => v instanceof Float64Array;
const isBigInt64Array = (v) => typeof BigInt64Array !== "undefined" && v instanceof BigInt64Array;
const isBigUint64Array = (v) => typeof BigUint64Array !== "undefined" && v instanceof BigUint64Array;
const isGeneratorFunction = (v) => typeof v === "function" && v.constructor && v.constructor.name === "GeneratorFunction";
const isGeneratorObject = (v) => v != null && typeof v.next === "function" && typeof v.throw === "function";
const isAsyncFunction = (v) => typeof v === "function" && v.constructor && v.constructor.name === "AsyncFunction";
const isMapIterator = () => false;
const isSetIterator = () => false;
const isStringObject = (v) => typeof v === "object" && v instanceof String;
const isNumberObject = (v) => typeof v === "object" && v instanceof Number;
const isBooleanObject = (v) => typeof v === "object" && v instanceof Boolean;
const isSymbolObject = () => false;
const isBoxedPrimitive = (v) => isStringObject(v) || isNumberObject(v) || isBooleanObject(v);
const isAnyArrayBuffer = (v) => isArrayBuffer(v) || isSharedArrayBuffer(v);
const isArrayBufferView = (v) => ArrayBuffer.isView(v);
const isArgumentsObject = () => false;
const isProxy = () => false;
const isModuleNamespaceObject = () => false;
const isExternal = () => false;

const types = {
  isDate, isRegExp, isPromise, isArray, isTypedArray, isUint8Array,
  isArrayBuffer, isSharedArrayBuffer, isNativeError, isSet, isMap,
  isWeakMap, isWeakSet, isDataView, isInt8Array, isUint8ClampedArray,
  isInt16Array, isUint16Array, isInt32Array, isUint32Array,
  isFloat32Array, isFloat64Array, isBigInt64Array, isBigUint64Array,
  isGeneratorFunction, isGeneratorObject, isAsyncFunction,
  isMapIterator, isSetIterator, isStringObject, isNumberObject,
  isBooleanObject, isSymbolObject, isBoxedPrimitive, isAnyArrayBuffer,
  isArrayBufferView, isArgumentsObject, isProxy, isModuleNamespaceObject,
  isExternal,
};
types.default = types;

export default types;
export {
  isDate, isRegExp, isPromise, isArray, isTypedArray, isUint8Array,
  isArrayBuffer, isSharedArrayBuffer, isNativeError, isSet, isMap,
  isWeakMap, isWeakSet, isDataView, isInt8Array, isUint8ClampedArray,
  isInt16Array, isUint16Array, isInt32Array, isUint32Array,
  isFloat32Array, isFloat64Array, isBigInt64Array, isBigUint64Array,
  isGeneratorFunction, isGeneratorObject, isAsyncFunction,
  isMapIterator, isSetIterator, isStringObject, isNumberObject,
  isBooleanObject, isSymbolObject, isBoxedPrimitive, isAnyArrayBuffer,
  isArrayBufferView, isArgumentsObject, isProxy, isModuleNamespaceObject,
  isExternal,
};
