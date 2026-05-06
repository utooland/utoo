module.exports = [
"[project]/target_node/onelogger_named_reexport/input/node_modules/onelogger/dist/esm/Logger.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

class Logger {
    static realLoggers = new Map();
    static setRealLogger(loggerName, realLogger) {
        this.realLoggers.set(loggerName, realLogger);
    }
    constructor(options){
        this.loggerName = options.loggerName;
        this.prefix = options.prefix;
    }
    info(message) {
        const realLogger = Logger.realLoggers.get(this.loggerName);
        realLogger?.info(`${this.prefix}:${message}`);
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
const loggers = new Map();
function setCustomLogger(loggerName, realLogger) {
    __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$onelogger_named_reexport$2f$input$2f$node_modules$2f$onelogger$2f$dist$2f$esm$2f$Logger$2e$js__$5b$server$5d$__$28$ecmascript$29$__["Logger"].setRealLogger(loggerName, realLogger);
}
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
__turbopack_context__.s([
    "getCustomLogger",
    0,
    getCustomLogger,
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
const logger = (0, __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$onelogger_named_reexport$2f$input$2f$node_modules$2f$onelogger$2f$dist$2f$esm$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__["getCustomLogger"])("worker");
logger.info("ready");
console.log("logger", logger.constructor.name);
__turbopack_context__.s([]);
}),
];

//# sourceMappingURL=_project__target_node_onelogger_named_reexport_input_38e7183c.js.map