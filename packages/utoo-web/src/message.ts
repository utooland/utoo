export const HandShake = "__handshake__";
export const Fork = "__fork__";

export enum SWMessageType {
  HandShake = 10000,
  HeartbeatPing,
  HeartbeatPong,
}

/** @deprecated use SWMessageType */
export const ServiceWorkerHandShake = SWMessageType.HandShake;
/** @deprecated use SWMessageType */
export const ServiceWorkerHeartbeatPing = SWMessageType.HeartbeatPing;
/** @deprecated use SWMessageType */
export const ServiceWorkerHeartbeatPong = SWMessageType.HeartbeatPong;
