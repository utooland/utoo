import os from "os";
import pc from "picocolors";

const BOX_INNER_WIDTH = 50;

export function printServerInfo(
  protocol: "http" | "https",
  hostname: string,
  port: number,
) {
  const localUrl = `${protocol}://localhost:${port}`;
  const networkAddress = getNetworkAddress();
  const networkUrl = networkAddress
    ? `${protocol}://${networkAddress}:${port}`
    : null;

  console.log();
  console.log(
    pc.green("  ┌──────────────────────────────────────────────────┐"),
  );
  printPlainBoxLine();
  printStyledBoxLine("   Serving!", `   ${pc.bold("Serving!")}`);
  printPlainBoxLine();
  printStyledBoxLine(
    `   - Local:    ${localUrl}`,
    `   - Local:    ${pc.cyan(localUrl)}`,
  );

  if (networkUrl) {
    printStyledBoxLine(
      `   - Network:  ${networkUrl}`,
      `   - Network:  ${pc.cyan(networkUrl)}`,
    );
  }

  printPlainBoxLine();
  printPlainBoxLine("   Copied local address to clipboard!");
  printPlainBoxLine();
  console.log(
    pc.green("  └──────────────────────────────────────────────────┘"),
  );
  console.log();
}

function printPlainBoxLine(content = "") {
  console.log(pc.green(`  │${content.padEnd(BOX_INNER_WIDTH, " ")}│`));
}

function printStyledBoxLine(plainContent: string, styledContent: string) {
  console.log(
    pc.green("  │") +
      styledContent +
      pc.green(
        `${" ".repeat(Math.max(0, BOX_INNER_WIDTH - plainContent.length))}│`,
      ),
  );
}

function getNetworkAddress() {
  const interfaces = os.networkInterfaces();
  for (const name of Object.keys(interfaces)) {
    for (const iface of interfaces[name]!) {
      if (iface.family === "IPv4" && !iface.internal) {
        return iface.address;
      }
    }
  }
  return undefined;
}
