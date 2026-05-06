import { appConfig } from './config.js';
import { getTernIndexAppProps as readFromSdk } from './sdk.js';

appConfig;

export function getTernIndexAppProps() {
  return readFromSdk();
}
