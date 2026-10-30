import fs from 'node:fs';
import path from 'node:path';

/**
 * Final artifacts owned by one browser scenario run.
 *
 * Only these names are removed between runs; unrelated files in a reused
 * output directory stay untouched.
 * 单次浏览器场景运行拥有的最终产物。
 *
 * 跨运行只删除这些名称；复用输出目录中的其他文件保持不变。
 */
export const MANAGED_ARTIFACTS = [
  'dom.json',
  'a11y.json',
  'reactive.json',
  'trace.json',
  'result.json',
  'browser.json',
];

/**
 * Point-in-time captures owned by one browser scenario run.
 * 单次浏览器场景运行拥有的时点捕获目录。
 */
export const MANAGED_CAPTURE_DIR = 'captures';

/**
 * Removes the previous run's managed artifacts before a new run starts.
 *
 * A failed rerun or a run that emits fewer artifacts must not leave stale
 * `result.json`, snapshots, browser checks, or capture steps readable. Missing
 * paths are ignored, and files outside the managed names are not touched.
 * 在新运行开始前删除上一次运行的受管产物。
 *
 * 失败重跑或产物更少的运行不得留下可读的旧 `result.json`、快照、浏览器检查或
 * 捕获步骤。缺失路径会被忽略，受管名称之外的文件不会被触碰。
 *
 * @param {string} outDir Absolute output directory that may be reused.
 */
export function cleanManagedArtifacts(outDir) {
  for (const name of MANAGED_ARTIFACTS) {
    fs.rmSync(path.join(outDir, name), { force: true, recursive: true });
  }
  fs.rmSync(path.join(outDir, MANAGED_CAPTURE_DIR), {
    force: true,
    recursive: true,
  });
}
