export async function xcodeProfilingReady() {
  await new Promise<void>((resolve) => {
    const readline = require("readline");
    const rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
    });
    rl.question(
      `Xcode profile enabled. Current process ${process.title} (${process.pid}) . Press Enter to continue...\n`,
      () => {
        rl.close();
        resolve();
      },
    );
  });
}
