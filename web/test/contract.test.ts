import test from 'node:test';
import { runConformance } from './conformance.ts';
test('S0 common codec, crypto and wallet boundaries', () => {
  const result = runConformance(); console.log(JSON.stringify({ runtime: 'Node', ...result }));
});
