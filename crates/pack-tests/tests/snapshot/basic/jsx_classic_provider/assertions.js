const assert = require("node:assert/strict");

module.exports = function verify(library) {
  const expected = {
    bareJsx: ["provider", "section", "jsx"],
    fragmentJsx: ["provider", "provider-fragment", "jsx fragment"],
    bareTsx: ["provider", "section", "tsx"],
    fragmentTsx: ["provider", "provider-fragment", "tsx fragment"],
    imported: ["imported", "section", "imported binding"],
    typeImport: ["provider", "section", "type import"],
    typeNamespace: ["provider", "section", "type namespace"],
    valueNamespace: ["namespace", "section", "value namespace"],
    ambient: ["provider", "section", "ambient declaration"],
    custom: ["custom", "custom-fragment", "custom pragma"],
    automatic: ["automatic", "section", "automatic directive"],
  };
  for (const [name, [factory, type, child]] of Object.entries(expected)) {
    assert.deepEqual(
      JSON.parse(JSON.stringify(library[name])),
      { factory, type, children: [child] },
      name,
    );
  }
  assert.equal(library.commonJS, 42);
};
