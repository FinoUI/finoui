import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { pathToFileURL } from 'node:url';

import { pkgFileFor, resolveWithin, webDirFrom } from './paths.mjs';

/**
 * Builds a module URL for a checkout path containing spaces and non-ASCII text.
 * 为包含空格与非 ASCII 文本的检出路径构造模块 URL。
 */
function moduleUrl(fixtureRoot, fileName) {
  return pathToFileURL(path.join(fixtureRoot, 'fino counter 测试', 'web', fileName))
    .href;
}

test('webDirFrom decodes percent escapes for spaces and non-ASCII paths', () => {
  const fixtureRoot = path.join(os.tmpdir(), 'fino paths');
  const url = moduleUrl(fixtureRoot, 'driver.mjs');

  assert.notEqual(url, url.replace(/%20/g, ' '));
  assert.equal(
    webDirFrom(url),
    path.join(fixtureRoot, 'fino counter 测试', 'web'),
  );
});

test('resolveWithin accepts package files and rejects root or escapes', () => {
  const root = path.join(os.tmpdir(), 'fino pkg');

  assert.equal(
    resolveWithin(root, 'fino_counter.js'),
    path.join(root, 'fino_counter.js'),
  );
  assert.equal(resolveWithin(root, ''), null);
  assert.equal(resolveWithin(root, '../secret.txt'), null);
  assert.equal(resolveWithin(root, 'nested/../../secret.txt'), null);
  assert.equal(resolveWithin(root, path.join(path.sep, 'etc', 'passwd')), null);
  assert.equal(resolveWithin(root, 'nested/ok.wasm'), path.join(root, 'nested/ok.wasm'));
});

test('pkgFileFor accepts normal files and rejects encoded traversal', () => {
  const root = path.join(os.tmpdir(), 'fino pkg');

  assert.equal(
    pkgFileFor(root, '/pkg/fino_counter.js'),
    path.join(root, 'fino_counter.js'),
  );
  assert.equal(
    pkgFileFor(root, '/pkg/nested/fino_counter_bg.wasm'),
    path.join(root, 'nested/fino_counter_bg.wasm'),
  );
  assert.equal(pkgFileFor(root, '/pkg/%2e%2e%2fsecret.txt'), null);
  assert.equal(pkgFileFor(root, '/pkg/nested/%2e%2e/%2e%2e/etc/passwd'), null);
  assert.equal(pkgFileFor(root, '/pkg//etc/passwd'), null);
  assert.equal(pkgFileFor(root, '/pkg/%2e%2e'), null);
  assert.equal(pkgFileFor(root, '/pkg/%ZZ'), null);
  assert.equal(pkgFileFor(root, '/index.html'), null);
});
