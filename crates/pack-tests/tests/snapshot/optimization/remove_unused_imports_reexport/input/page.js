// Async loaded page that uses transformProxyUrl
import { transformProxyUrl } from './tern/sdk';

export default function Page() {
  const url = transformProxyUrl('/api/users');
  console.log('Transformed URL:', url);
  return { url };
}

