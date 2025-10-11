export default function lazyModule() {
  console.log('Lazy module loaded from CDN!');
  return { loaded: true, source: 'CDN' };
}

export const lazyData = {
  message: 'This chunk should be loaded from publicPath'
};

