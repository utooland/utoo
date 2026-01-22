import { DOMParser } from "domparser-rs";
import fs from "fs";
import path from "path";
import { ConfigComplete, EntryOptions } from "../config/types";

export function processHtmlEntry(config: ConfigComplete, projectPath: string) {
  if (!config.entry) return;

  const newEntries: EntryOptions[] = [];

  config.entry = config.entry.filter((entry) => {
    if (entry.import.endsWith(".html")) {
      const htmlPath = path.resolve(projectPath, entry.import);
      if (fs.existsSync(htmlPath)) {
        const content = fs.readFileSync(htmlPath, "utf-8");

        const parser = new DOMParser();
        const doc = parser.parseFromString(content, "text/html");
        const scripts = doc.querySelectorAll("script");

        scripts.forEach((script) => {
          const src = script.getAttribute("src");
          const type = script.getAttribute("type");
          if (
            src &&
            !src.startsWith("http") &&
            !src.startsWith("//") &&
            type === "module"
          ) {
            const scriptPath = path.join(path.dirname(entry.import), src);
            // Remove the origin script tag from the DOM
            if (script.parentNode) {
              script.parentNode.removeChild(script);
            }

            newEntries.push({
              import: scriptPath,
              html: {
                template: entry.import,
                templateContent: doc.outerHTML,
                filename: path.basename(entry.import),
              },
            });
          }
        });

        return false;
      }
    }
    return true;
  });

  // Add new script entries
  config.entry.push(...newEntries);
}
