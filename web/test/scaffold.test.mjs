import test from 'node:test';
import assert from 'node:assert/strict';
import { stage } from '../dist/index.js';
test('compiled ESM entrypoint loads', () => assert.equal(stage, 'S0-scaffold'));
