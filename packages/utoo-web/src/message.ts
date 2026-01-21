export enum WorkerMessageType {
  InitConnection = 0x574d0000, // "WM" in hex
  RequestFork,
}

/** @deprecated use WorkerMessageType.InitConnection */
export const HandShake = WorkerMessageType.InitConnection;
/** @deprecated use WorkerMessageType.RequestFork */
export const Fork = WorkerMessageType.RequestFork;

export enum SWMessageType {
  HandShake = 0x53570000, // "SW" in hex
  HeartbeatPing,
  HeartbeatPong,
}

/** @deprecated use SWMessageType */
export const ServiceWorkerHandShake = SWMessageType.HandShake;
/** @deprecated use SWMessageType */
export const ServiceWorkerHeartbeatPing = SWMessageType.HeartbeatPing;
/** @deprecated use SWMessageType */
export const ServiceWorkerHeartbeatPong = SWMessageType.HeartbeatPong;
