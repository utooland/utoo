const value = 42;

function getMessage() {
  return 'Hello from Node.js platform';
}

console.log(getMessage(), value);

module.exports = { value, getMessage };
