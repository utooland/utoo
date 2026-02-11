'use strict';

module.exports = appInfo => {
  const config = {};

  config.keys = appInfo.name + '_utoo_runtime_demo';

  // Disable security features for demo simplicity
  config.security = {
    csrf: {
      enable: false,
    },
  };

  // Disable logger file transport for runtime compatibility
  config.logger = {
    dir: '/tmp/egg-hello-logs',
    level: 'INFO',
    consoleLevel: 'INFO',
  };

  return config;
};
