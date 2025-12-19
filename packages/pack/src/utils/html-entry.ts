import { DOMParser } from "domparser-rs";
import fs from "fs";
import path from "path";
import { ConfigComplete, EntryOptions } from "../config/types";

export function processHtmlEntry(config: ConfigComplete, projectPath: string) {
  if (!config.entry) return;

  const newEntries: EntryOptions[] = [];

  const entriesToRemove: number[] = [];

  config.entry.forEach((entry, index) => {
    if (entry.import.endsWith(".html")) {
      const htmlPath = path.resolve(projectPath, entry.import);
      if (fs.existsSync(htmlPath)) {
        const content = fs.readFileSync(htmlPath, "utf-8");

        const parser = new DOMParser();
        const doc = parser.parseFromString(content, "text/html");
        const scripts = doc.querySelectorAll("script");

        scripts.forEach((script) => {
          const src = script.getAttribute("src");
          if (src && !src.startsWith("http") && !src.startsWith("//")) {
            const scriptPath = path.join(path.dirname(entry.import), src);
            newEntries.push({
              import: scriptPath,
              html: {
                template: entry.import,
                templateContent: doc.outerHTML,
                filename: path.basename(entry.import),
              },
            });
            // Remove the script tag from the DOM
            if (script.parentNode) {
              script.parentNode.removeChild(script);
            }
          }
        });

        entriesToRemove.push(index);
      }
    }
  });

  // Remove processed HTML entries (in reverse order to maintain indices)
  for (let i = entriesToRemove.length - 1; i >= 0; i--) {
    config.entry.splice(entriesToRemove[i], 1);
  }

  // Add new script entries
  config.entry.push(...newEntries);
}
