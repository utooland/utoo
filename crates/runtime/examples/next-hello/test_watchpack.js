const Watchpack = require('watchpack');

const appDir = '/Users/elr/code/utoo/crates/runtime/examples/next-hello/app';
const pagesDir = '/Users/elr/code/utoo/crates/runtime/examples/next-hello/pages';

const wp = new Watchpack({
  aggregateTimeout: 5,
  ignored: () => false
});

console.log('watchpack created');

wp.on('aggregated', () => {
  console.log('=== AGGREGATED EVENT ===');
  const knownFiles = wp.getTimeInfoEntries();
  console.log('knownFiles size:', knownFiles.size);
  for (const [key, val] of knownFiles) {
    console.log(' ', key, '->', JSON.stringify(val));
  }
});

wp.on('change', (file, mtime) => {
  console.log('change:', file, mtime);
});

console.log('starting watch...');
wp.watch({
  directories: [appDir, pagesDir],
  files: [],
  startTime: 0
});
console.log('watch started');

// Wait for aggregation
setTimeout(() => {
  console.log('\n=== AFTER 3s ===');
  const knownFiles = wp.getTimeInfoEntries();
  console.log('knownFiles size:', knownFiles.size);
  for (const [key, val] of knownFiles) {
    console.log(' ', key, '->', JSON.stringify(val));
  }
  wp.close();
  process.exit(0);
}, 3000);
