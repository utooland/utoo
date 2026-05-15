import { DOMParser } from "domparser-rs";
import fs from "fs";
import path from "path";
import { HtmlConfig } from "../config/types";

function normalizeHtmlPublicPath(globalPublicPath: string | undefined): string {
  return globalPublicPath === "runtime" || globalPublicPath === "auto"
    ? ""
    : (globalPublicPath ?? "");
}

export class HtmlPlugin {
  constructor(private config: HtmlConfig) {}

  async generate(
    outputDir: string,
    assets: { js: string[]; css: string[] },
    globalPublicPath?: string,
  ) {
    const templatePath = this.config.template
      ? path.resolve(process.cwd(), this.config.template)
      : undefined;

    const publicPath = normalizeHtmlPublicPath(globalPublicPath);

    let htmlContent = "";
    if (this.config.templateContent) {
      htmlContent = this.config.templateContent;
    } else if (templatePath && fs.existsSync(templatePath)) {
      htmlContent = fs.readFileSync(templatePath, "utf-8");
    } else {
      htmlContent = `<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8">
    <title>${this.config.title || "Utoo App"}</title>
  </head>
  <body>
    <div id="root"></div>
  </body>
</html>`;
    }

    const parser = new DOMParser();
    const doc = parser.parseFromString(htmlContent, "text/html");

    if (this.config.title) {
      const title = doc.querySelector("title");
      if (title) {
        title.textContent = this.config.title;
      } else {
        const head = doc.querySelector("head");
        if (head) {
          const titleNode = doc.createElement("title");
          titleNode.textContent = this.config.title;
          head.appendChild(titleNode);
        }
      }
    }

    // Inject meta
    if (this.config.meta) {
      const head = doc.querySelector("head");
      if (head) {
        Object.entries(this.config.meta).forEach(([name, value]) => {
          const meta = doc.createElement("meta");
          if (typeof value === "string") {
            meta.setAttribute("name", name);
            meta.setAttribute("content", value);
          } else {
            Object.entries(value).forEach(([k, v]) => {
              meta.setAttribute(k, v);
            });
          }
          head.appendChild(meta);
        });
      }
    }

    const head = doc.querySelector("head");
    const body = doc.querySelector("body");

    // Inject CSS
    assets.css.forEach((cssFile) => {
      const link = doc.createElement("link");
      link.setAttribute("rel", "stylesheet");
      link.setAttribute(
        "href",
        publicPath
          ? publicPath.endsWith("/")
            ? publicPath + cssFile
            : publicPath + "/" + cssFile
          : cssFile,
      );
      if (head) head.appendChild(link);
    });

    // Inject JS
    assets.js.forEach((jsFile) => {
      const script = doc.createElement("script");
      script.setAttribute(
        "src",
        publicPath
          ? publicPath.endsWith("/")
            ? publicPath + jsFile
            : publicPath + "/" + jsFile
          : jsFile,
      );
      if (this.config.scriptLoading === "defer") {
        script.setAttribute("defer", "");
      } else if (this.config.scriptLoading === "module") {
        script.setAttribute("type", "module");
      }

      if (this.config.inject === "head" && head) {
        head.appendChild(script);
      } else if (body) {
        body.appendChild(script);
      } else if (head) {
        head.appendChild(script); // Fallback
      }
    });

    const finalHtml = doc.outerHTML;
    const filename = this.config.filename || "index.html";
    const finalOutputDir = this.config.output?.path ?? outputDir;
    fs.mkdirSync(finalOutputDir, { recursive: true });
    fs.writeFileSync(path.join(finalOutputDir, filename), finalHtml);
  }
}
