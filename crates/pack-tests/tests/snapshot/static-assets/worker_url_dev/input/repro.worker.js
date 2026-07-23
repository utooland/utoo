import { getMessage } from "./worker-message.js";

self.postMessage(getMessage());
