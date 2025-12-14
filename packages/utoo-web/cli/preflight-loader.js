const fs = require('fs');

module.exports = function(source) {
  try {
    // Resolve the path to preflight.css using the node resolution mechanism
    // We assume tailwindcss is installed where this build runs
    const preflightPath = require.resolve('tailwindcss/lib/css/preflight.css');
    const css = fs.readFileSync(preflightPath, 'utf8');
    
    // Escape the CSS content to be a valid JS string literal
    const escapedCss = JSON.stringify(css);
    
    // Replace the specific readFileSync call
    // Pattern: _fs.default.readFileSync(_path.join(__dirname, "./css/preflight.css"), "utf8")
    // We use a regex to be slightly more robust against whitespace, though the file is likely generated/minified consistently.
    const pattern = /_fs\.default\.readFileSync\(_path\.join\(__dirname, "\.\/css\/preflight\.css"\), "utf8"\)/g;
    
    if (pattern.test(source)) {
      console.log('✅ [preflight-loader] Injected preflight.css content into corePlugins.js');
      return source.replace(pattern, escapedCss);
    } else {
      console.warn('⚠️ [preflight-loader] Could not find the readFileSync pattern in corePlugins.js');
      return source;
    }
  } catch (e) {
    console.error('❌ [preflight-loader] Error injecting preflight.css:', e);
    return source;
  }
};
