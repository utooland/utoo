import { appConfig } from './config.js';

export function transformProxyUrl(originPath) {
  var ternAppConfig = appConfig;

  return ternAppConfig.name + ':' + originPath;
}

export function getTernIndexAppProps() {
  var yuyanId = appConfig.yuyanId, appName = appConfig.name;

  return appName + ':' + yuyanId;
}

export function unusedTernExport() {
  return 'unused';
}
