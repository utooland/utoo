module.exports = [
"[externals]/node:assert [external] (node:assert, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("node:assert", () => require("node:assert"));

module.exports = mod;
}),
"[externals]/node:util [external] (node:util, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("node:util", () => require("node:util"));

module.exports = mod;
}),
"[project]/target_node/onelogger_named_reexport/input/node_modules/onelogger/dist/esm/Logger.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$assert__$5b$external$5d$__$28$node$3a$assert$2c$__cjs$29$__ = __turbopack_context__.i("[externals]/node:assert [external] (node:assert, cjs)");
var __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$util__$5b$external$5d$__$28$node$3a$util$2c$__cjs$29$__ = __turbopack_context__.i("[externals]/node:util [external] (node:util, cjs)");
;
;
if (!globalThis.__ONE_LOGGER_INSTANCES__) {
    globalThis.__ONE_LOGGER_INSTANCES__ = new Map();
}
class Logger {
    #loggerName;
    #prefix;
    constructor(options){
        this.#loggerName = options.loggerName;
        this.#prefix = options.prefix;
    }
    info(message, ...optionalParams) {
        this.#log("info", message, ...optionalParams);
    }
    warn(message, ...optionalParams) {
        this.#log("warn", message, ...optionalParams);
    }
    error(message, ...optionalParams) {
        this.#log("error", message, ...optionalParams);
    }
    #log(level, message, ...optionalParams) {
        const realLogger = this._getRealLogger();
        if (this.#prefix) {
            const log = (0, __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$util__$5b$external$5d$__$28$node$3a$util$2c$__cjs$29$__["format"])(message, ...optionalParams);
            realLogger[level](`[${this.#prefix}] ${log}`);
        } else {
            realLogger[level](message, ...optionalParams);
        }
    }
    _getRealLogger() {
        return globalThis.__ONE_LOGGER_INSTANCES__.get(this.#loggerName) ?? globalThis.console;
    }
    static setRealLogger(loggerName, realLogger) {
        if (!realLogger) {
            globalThis.__ONE_LOGGER_INSTANCES__.delete(loggerName);
        } else {
            (0, __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$assert__$5b$external$5d$__$28$node$3a$assert$2c$__cjs$29$__["strict"])(!(realLogger instanceof Logger), "can't set realLogger to Logger instance");
            globalThis.__ONE_LOGGER_INSTANCES__.set(loggerName, realLogger);
        }
    }
}
__turbopack_context__.s([
    "Logger",
    0,
    Logger
]);
}),
"[project]/target_node/onelogger_named_reexport/input/node_modules/onelogger/dist/esm/index.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$onelogger_named_reexport$2f$input$2f$node_modules$2f$onelogger$2f$dist$2f$esm$2f$Logger$2e$js__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/target_node/onelogger_named_reexport/input/node_modules/onelogger/dist/esm/Logger.js [server] (ecmascript)");
;
;
function setCustomLogger(loggerName, realLogger) {
    __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$onelogger_named_reexport$2f$input$2f$node_modules$2f$onelogger$2f$dist$2f$esm$2f$Logger$2e$js__$5b$server$5d$__$28$ecmascript$29$__["Logger"].setRealLogger(loggerName, realLogger);
}
function setLogger(realLogger) {
    setCustomLogger("logger", realLogger);
}
function setCoreLogger(realLogger) {
    setCustomLogger("coreLogger", realLogger);
}
const loggers = new Map();
function getCustomLogger(loggerName, prefix) {
    const key = `${loggerName}-${prefix ?? ""}`;
    let logger = loggers.get(key);
    if (!logger) {
        logger = new __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$onelogger_named_reexport$2f$input$2f$node_modules$2f$onelogger$2f$dist$2f$esm$2f$Logger$2e$js__$5b$server$5d$__$28$ecmascript$29$__["Logger"]({
            loggerName,
            prefix
        });
        loggers.set(key, logger);
    }
    return logger;
}
function getLogger(prefix) {
    return getCustomLogger("logger", prefix);
}
function getCoreLogger(prefix) {
    return getCustomLogger("coreLogger", prefix);
}
__turbopack_context__.s([
    "setCustomLogger",
    0,
    setCustomLogger
]);
}),
"[project]/target_node/onelogger_named_reexport/input/index.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$onelogger_named_reexport$2f$input$2f$node_modules$2f$onelogger$2f$dist$2f$esm$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/target_node/onelogger_named_reexport/input/node_modules/onelogger/dist/esm/index.js [server] (ecmascript)");
;
(0, __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$onelogger_named_reexport$2f$input$2f$node_modules$2f$onelogger$2f$dist$2f$esm$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__["setCustomLogger"])("logger", {
    info (message) {
        console.log(`real:${message}`);
    }
});
__turbopack_context__.s([]);
}),
];

//# sourceMappingURL=_root-of-the-server___4360d624.js.map