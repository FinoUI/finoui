import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  MANAGED_ARTIFACTS,
  MANAGED_CAPTURE_DIR,
  cleanManagedArtifacts,
} from './artifacts.mjs';

/**
 * Creates an isolated output directory for artifact cleanup tests.
 * 为产物清理测试创建隔离的输出目录。
 */
function fixtureDirectory(name) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `fino ${name} `));
}

/**
 * Writes a file and creates its parent directories.
 * 写入文件并创建其父目录。
 */
function write(file, contents = 'stale') {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, contents);
}

test('cleanManagedArtifacts removes only managed files and captures', () => {
  const outDir = fixtureDirectory('artifacts');
  for (const name of MANAGED_ARTIFACTS) {
    write(path.join(outDir, name));
  }
  write(path.join(outDir, MANAGED_CAPTURE_DIR, 'step-4', 'dom.json'));
  write(path.join(outDir, 'layout.json'), 'not managed');
  write(path.join(outDir, 'keep.txt'), 'keep me');
  write(path.join(outDir, 'notes', 'browser.json'), 'nested, not managed');

  cleanManagedArtifacts(outDir);

  for (const name of MANAGED_ARTIFACTS) {
    assert.equal(fs.existsSync(path.join(outDir, name)), false, name);
  }
  assert.equal(fs.existsSync(path.join(outDir, MANAGED_CAPTURE_DIR)), false);
  assert.equal(fs.readFileSync(path.join(outDir, 'layout.json'), 'utf8'), 'not managed');
  assert.equal(fs.readFileSync(path.join(outDir, 'keep.txt'), 'utf8'), 'keep me');
  assert.equal(
    fs.readFileSync(path.join(outDir, 'notes', 'browser.json'), 'utf8'),
    'nested, not managed',
  );
});

test('cleanManagedArtifacts tolerates omitted artifacts and repeated cleanup', () => {
  const outDir = fixtureDirectory('omitted');
  write(path.join(outDir, 'result.json'));
  write(path.join(outDir, 'keep.txt'), 'unrelated');

  cleanManagedArtifacts(outDir);
  cleanManagedArtifacts(outDir);

  assert.equal(fs.existsSync(path.join(outDir, 'result.json')), false);
  assert.equal(fs.readFileSync(path.join(outDir, 'keep.txt'), 'utf8'), 'unrelated');
});

test('cleanManagedArtifacts leaves unrelated files in a fresh directory', () => {
  const outDir = fixtureDirectory('fresh');
  write(path.join(outDir, 'keep.txt'), 'untouched');

  cleanManagedArtifacts(outDir);

  assert.equal(fs.readFileSync(path.join(outDir, 'keep.txt'), 'utf8'), 'untouched');
});
