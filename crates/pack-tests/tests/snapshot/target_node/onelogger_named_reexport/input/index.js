import { setCustomLogger } from "onelogger";

setCustomLogger("logger", {
  info(message) {
    console.log(`real:${message}`);
  },
});
