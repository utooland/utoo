import os from "os";
import pc from "picocolors";

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
  console.log(
    pc.green("  │                                                  │"),
  );
  console.log(
    pc.green("  │   ") +
      pc.bold("Serving!") +
      pc.green("                                       │"),
  );
  console.log(
    pc.green("  │                                                  │"),
  );

  const localLabel = "  │   - Local:    ";
  const localPadding = 50 - 15 - localUrl.length; // 15 is "   - Local:    ".length
  console.log(
    pc.green(localLabel) +
      pc.cyan(localUrl) +
      pc.green(" ".repeat(Math.max(0, localPadding)) + "│"),
  );

  if (networkUrl) {
    const netLabel = "  │   - Network:  ";
    const netPadding = 50 - 17 - networkUrl.length; // 17 is "   - Network:  ".length
    console.log(
      pc.green(netLabel) +
        pc.cyan(networkUrl) +
        pc.green(" ".repeat(Math.max(0, netPadding)) + "│"),
    );
  }

  console.log(
    pc.green("  │                                                  │"),
  );
  console.log(
    pc.green("  │   Copied local address to clipboard!             │"),
  );
  console.log(
    pc.green("  │                                                  │"),
  );
  console.log(
    pc.green("  └──────────────────────────────────────────────────┘"),
  );
  console.log();
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
