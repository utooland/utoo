import { execSync } from "node:child_process";
import { createPrivateKey, X509Certificate } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import os from "os";

const { WritableStream } = require("node:stream/web") as {
  WritableStream: typeof global.WritableStream;
};

const MKCERT_VERSION = "v1.4.4";

export interface SelfSignedCertificate {
  key: string;
  cert: string;
  rootCA?: string;
}

function getBinaryName() {
  const platform = process.platform;
  const arch = process.arch === "x64" ? "amd64" : process.arch;

  if (platform === "win32") {
    return `mkcert-${MKCERT_VERSION}-windows-${arch}.exe`;
  }
  if (platform === "darwin") {
    return `mkcert-${MKCERT_VERSION}-darwin-${arch}`;
  }
  if (platform === "linux") {
    return `mkcert-${MKCERT_VERSION}-linux-${arch}`;
  }

  throw new Error(`Unsupported platform: ${platform}`);
}

async function downloadBinary() {
  try {
    const binaryName = getBinaryName();
    const cacheDirectory = getCacheDirectory("mkcert");
    const binaryPath = path.join(cacheDirectory, binaryName);

    if (fs.existsSync(binaryPath)) {
      return binaryPath;
    }

    const downloadUrl = `https://github.com/FiloSottile/mkcert/releases/download/${MKCERT_VERSION}/${binaryName}`;

    await fs.promises.mkdir(cacheDirectory, { recursive: true });

    console.log(`Downloading mkcert package...`);

    const response = await fetch(downloadUrl);

    if (!response.ok || !response.body) {
      throw new Error(`request failed with status ${response.status}`);
    }

    console.log(`Download response was successful, writing to disk`);

    const binaryWriteStream = fs.createWriteStream(binaryPath);

    await response.body.pipeTo(
      new WritableStream({
        write(chunk) {
          return new Promise((resolve, reject) => {
            binaryWriteStream.write(chunk, (error) => {
              if (error) {
                reject(error);
                return;
              }

              resolve();
            });
          });
        },
        close() {
          return new Promise((resolve, reject) => {
            binaryWriteStream.close((error) => {
              if (error) {
                reject(error);
                return;
              }

              resolve();
            });
          });
        },
      }),
    );

    await fs.promises.chmod(binaryPath, 0o755);

    return binaryPath;
  } catch (err) {
    console.error("Error downloading mkcert:", err);
  }
}

export async function createSelfSignedCertificate(
  host?: string,
  certDir: string = "certificates",
): Promise<SelfSignedCertificate | undefined> {
  try {
    const binaryPath = await downloadBinary();
    if (!binaryPath) throw new Error("missing mkcert binary");

    const resolvedCertDir = path.resolve(process.cwd(), `./${certDir}`);

    await fs.promises.mkdir(resolvedCertDir, {
      recursive: true,
    });

    const keyPath = path.resolve(resolvedCertDir, "localhost-key.pem");
    const certPath = path.resolve(resolvedCertDir, "localhost.pem");

    if (fs.existsSync(keyPath) && fs.existsSync(certPath)) {
      const cert = new X509Certificate(fs.readFileSync(certPath));
      const key = fs.readFileSync(keyPath);

      if (
        cert.checkHost(host ?? "localhost") &&
        cert.checkPrivateKey(createPrivateKey(key))
      ) {
        console.log("Using already generated self signed certificate");
        const caLocation = execSync(`"${binaryPath}" -CAROOT`)
          .toString()
          .trim();

        return {
          key: keyPath,
          cert: certPath,
          rootCA: `${caLocation}/rootCA.pem`,
        };
      }
    }

    console.log(
      "Attempting to generate self signed certificate. This may prompt for your password",
    );

    const defaultHosts = ["localhost", "127.0.0.1", "::1"];

    const hosts =
      host && !defaultHosts.includes(host)
        ? [...defaultHosts, host]
        : defaultHosts;

    execSync(
      `"${binaryPath}" -install -key-file "${keyPath}" -cert-file "${certPath}" ${hosts.join(
        " ",
      )}`,
      { stdio: "ignore" },
    );

    const caLocation = execSync(`"${binaryPath}" -CAROOT`).toString().trim();

    if (!fs.existsSync(keyPath) || !fs.existsSync(certPath)) {
      throw new Error("Certificate files not found");
    }

    console.log(`CA Root certificate created in ${caLocation}`);
    console.log(`Certificates created in ${resolvedCertDir}`);

    const gitignorePath = path.resolve(process.cwd(), "./.gitignore");

    if (fs.existsSync(gitignorePath)) {
      const gitignore = await fs.promises.readFile(gitignorePath, "utf8");
      if (!gitignore.includes(certDir)) {
        console.log("Adding certificates to .gitignore");

        await fs.promises.appendFile(gitignorePath, `\n${certDir}`);
      }
    }

    return {
      key: keyPath,
      cert: certPath,
      rootCA: `${caLocation}/rootCA.pem`,
    };
  } catch (err) {
    console.error(
      "Failed to generate self-signed certificate. Falling back to http.",
      err,
    );
  }
}

// get platform specific cache directory adapted from playwright's handling
// https://github.com/microsoft/playwright/blob/7d924470d397975a74a19184c136b3573a974e13/packages/playwright-core/src/utils/registry.ts#L141
export function getCacheDirectory(fileDirectory: string, envPath?: string) {
  let result;

  if (envPath) {
    result = envPath;
  } else {
    let systemCacheDirectory;
    if (process.platform === "linux") {
      systemCacheDirectory =
        process.env.XDG_CACHE_HOME || path.join(os.homedir(), ".cache");
    } else if (process.platform === "darwin") {
      systemCacheDirectory = path.join(os.homedir(), "Library", "Caches");
    } else if (process.platform === "win32") {
      systemCacheDirectory =
        process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
    } else {
      /// Attempt to use generic tmp location for un-handled platform
      if (!systemCacheDirectory) {
        for (const dir of [
          path.join(os.homedir(), ".cache"),
          path.join(os.tmpdir()),
        ]) {
          if (fs.existsSync(dir)) {
            systemCacheDirectory = dir;
            break;
          }
        }
      }

      if (!systemCacheDirectory) {
        console.error(new Error("Unsupported platform: " + process.platform));
        process.exit(0);
      }
    }
    result = path.join(systemCacheDirectory, fileDirectory);
  }

  if (!path.isAbsolute(result)) {
    // It is important to resolve to the absolute path:
    //   - for unzipping to work correctly;
    //   - so that registry directory matches between installation and execution.
    // INIT_CWD points to the root of `npm/yarn install` and is probably what
    // the user meant when typing the relative path.
    result = path.resolve(process.env["INIT_CWD"] || process.cwd(), result);
  }
  return result;
}
