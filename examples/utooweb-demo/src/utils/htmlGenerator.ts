export const generateHtml = (styles: string[], scripts: string[]): string => {
    return `
<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Preview</title>
    ${styles.join("\n    ").trim()}
  </head>
  <body>
    <div id="root"></div>
    ${scripts.join("\n    ").trim()}
  </body>
</html>
    `;
};
