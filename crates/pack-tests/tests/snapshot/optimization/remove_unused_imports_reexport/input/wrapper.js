// Imports fnA from pkg and USES it
// Re-exports fnB from pkg
import { fnA } from 'pkg';

export { fnB } from 'pkg';

export function useFnA() {
  return fnA();
}

