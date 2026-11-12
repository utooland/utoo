import { serve } from "../commands/dev";

const directory = process.env.UTOO_SIGNAL_TEST_DIR;
if (!directory) throw new Error("Missing signal fixture directory");

await serve({ config: { entry: [] } }, directory, directory, {
  port: 0,
  hostname: "127.0.0.1",
  logServerInfo: false,
});

// onReady runs before the signal handlers are installed; wait for serve().
process.send?.("ready");
