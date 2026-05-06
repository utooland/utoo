import { getCustomLogger, setCustomLogger } from "onelogger";

setCustomLogger("logger", {
  info(message) {
    console.log(`real:${message}`);
  },
});

const logger = getCustomLogger("worker");
logger.info("ready");
console.log("logger", logger.constructor.name);
