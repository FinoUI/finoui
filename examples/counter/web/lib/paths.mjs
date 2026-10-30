import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Resolves the directory that contains the file named by an `import.meta.url`.
 *
 * `fileURLToPath` decodes percent escapes and converts Windows drive paths, so a
 * checkout containing spaces or non-ASCII characters still resolves correctly.
 * 解析 `import.meta.url` 所指文件所在目录。
 *
 * `fileURLToPath` 会解码百分号转义并转换 Windows 盘符路径，因此包含空格或
 * 非 ASCII 字符的检出目录也能正确解析。
 *
 * @param {string} importMetaUrl The current module's `import.meta.url`.
 * @returns {string} Absolute directory path containing the module.
 */
export function webDirFrom(importMetaUrl) {
  return path.dirname(fileURLToPath(importMetaUrl));
}

/**
 * Resolves a request path beneath a serving root and rejects escapes.
 *
 * Returns `null` for the root itself, absolute candidates, and candidates whose
 * relative path starts with `..`. Callers translate `null` into a 404 response.
 * 在服务根目录下解析请求路径，并拒绝逃逸。
 *
 * 对根目录本身、绝对路径候选以及相对路径以 `..` 开头的候选返回 `null`。
 * 调用方把 `null` 转换为 404 响应。
 *
 * @param {string} root Absolute serving root.
 * @param {string} relative Decoded path relative to the serving root.
 * @returns {string | null} Absolute candidate inside the root, or `null`.
 */
export function resolveWithin(root, relative) {
  if (typeof relative !== 'string' || relative.includes('\0')) {
    return null;
  }
  const candidate = path.resolve(root, relative);
  const relation = path.relative(root, candidate);
  if (
    relation === '' ||
    relation === '..' ||
    relation.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relation)
  ) {
    return null;
  }
  return candidate;
}

/**
 * Maps a raw `/pkg/` request pathname to a file inside the wasm package directory.
 *
 * Decoding happens before containment checks, so encoded traversal such as
 * `%2e%2e%2f` cannot escape the package root. Malformed escapes also return `null`.
 * 把原始 `/pkg/` 请求路径映射到 wasm 包目录内的文件。
 *
 * 解码先于包含性检查，因此 `%2e%2e%2f` 等编码路径穿越无法逃出包根目录；
 * 非法转义同样返回 `null`。
 *
 * @param {string} pkgDir Absolute wasm package directory.
 * @param {string} rawPathname Raw request pathname before percent decoding.
 * @returns {string | null} Absolute package file path, or `null` when the request escapes.
 */
export function pkgFileFor(pkgDir, rawPathname) {
  let pathname;
  try {
    pathname = decodeURIComponent(rawPathname);
  } catch {
    return null;
  }
  if (!pathname.startsWith('/pkg/')) {
    return null;
  }
  return resolveWithin(pkgDir, pathname.slice('/pkg/'.length));
}
