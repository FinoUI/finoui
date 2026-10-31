// Headless Chromium driver for the counter's wasm build.
// 计数器 wasm 构建的无头 Chromium 驱动。

import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

import { cleanManagedArtifacts } from './lib/artifacts.mjs';
import { pkgFileFor, webDirFrom } from './lib/paths.mjs';

let chromium;
const checks = [];
try {
  ({ chromium } = await import('playwright'));
} catch (error) {
  fail('UI_PLAYWRIGHT_MISSING', `playwright is not installed; run npm ci --prefix examples/counter/web (${error.message})`);
}

const args = parseArgs(process.argv.slice(2));

/**
 * Reads one required driver argument or reports a structured usage failure.
 * 读取一个必需的驱动参数，或报告结构化用法失败。
 *
 * @param {string} name Argument name without the leading `--`.
 * @returns {string} Argument value.
 */
function required(name) {
  const value = args.get(name);
  if (!value) {
    fail('UI_BROWSER_ARGUMENT', `missing required --${name} argument`);
  }
  return value;
}

/**
 * Parses strict `--name value` pairs without shell interpretation.
 *
 * Only the expected argument shape is accepted; unknown positional input and
 * missing values become structured failures instead of partial runs.
 * 解析严格的 `--name value` 参数对，不经过 shell 解释。
 *
 * 只接受预期参数形状；未知位置参数和缺失值会转为结构化失败，而不是部分运行。
 *
 * @param {string[]} argv Driver arguments after the script path.
 * @returns {Map<string, string>} Argument names mapped to values.
 */
function parseArgs(argv) {
  const parsed = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith('--')) {
      fail('UI_BROWSER_ARGUMENT', `unexpected argument ${argument}`);
    }
    const value = argv[index + 1];
    if (value === undefined) {
      fail('UI_BROWSER_ARGUMENT', `missing value for ${argument}`);
    }
    parsed.set(argument.slice(2), value);
    index += 1;
  }
  return parsed;
}

/**
 * Fails the current browser check when a semantic condition is false.
 * 当语义条件为假时使当前浏览器检查失败。
 *
 * @param {unknown} condition Condition that must be truthy.
 * @param {string} message Failure message shown in the structured report.
 */
function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

/**
 * Compares two values strictly and reports both sides on mismatch.
 * 严格比较两个值，并在不匹配时报告双方。
 *
 * @param {unknown} actual Observed value.
 * @param {unknown} expected Required value.
 * @param {string} label Field name included in the failure message.
 */
function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

/**
 * Runs one named browser check and records its outcome exactly once.
 *
 * A failure is recorded before rethrowing so the final report shows which
 * guarantee broke instead of only a generic driver error.
 * 运行单个具名浏览器检查并只记录一次结果。
 *
 * 失败会在重新抛出前记录，使最终报告指出被破坏的保证，而不只是通用驱动错误。
 *
 * @param {string} name Stable check name used in `browser.json`.
 * @param {() => Promise<void>} operation Check body.
 */
async function runCheck(name, operation) {
  try {
    await operation();
    checks.push({ name, status: 'passed' });
  } catch (error) {
    checks.push({ name, status: 'failed', message: error.message });
    throw error;
  }
}

/**
 * Writes a structured failure payload and sets a non-zero exit code.
 *
 * The browser and server still shut down through the surrounding `finally`
 * block, so failed checks do not leak Chromium processes.
 * 写入结构化失败负载并设置非零退出码。
 *
 * 浏览器与服务器仍会通过外层 `finally` 关闭，因此失败检查不会泄漏 Chromium 进程。
 *
 * @param {string} code Stable tool error code.
 * @param {string} message Human-readable failure message.
 * @param {unknown} [report] Optional scenario result retained for diagnostics.
 */
function emitFailure(code, message, report = undefined) {
  const payload = {
    status: 'failed',
    error: { code, message, severity: 'error', backend: 'web-headless' },
    checks,
    ...(report ? { result: report } : {}),
  };
  process.exitCode = 1;
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

/**
 * Reports a fatal argument or setup failure and exits immediately.
 * 报告致命参数或初始化失败并立即退出。
 *
 * @param {string} code Stable tool error code.
 * @param {string} message Human-readable failure message.
 * @param {unknown} [report] Optional scenario result retained for diagnostics.
 */
function fail(code, message, report = undefined) {
  emitFailure(code, message, report);
  process.exit(1);
}

/**
 * Clears previous managed outputs before any browser check or scenario runs.
 *
 * Only the declared managed artifacts and capture directory are removed, so
 * unrelated files in a reused directory survive a failed or partial rerun.
 * 在任何浏览器检查或场景运行前清除上一次的受管输出。
 *
 * 只删除声明的受管产物与捕获目录，因此复用目录中的无关文件会在失败或不完整重跑后保留。
 *
 * @param {string} directory Absolute output directory.
 */
function cleanOutDir(directory) {
  try {
    cleanManagedArtifacts(directory);
  } catch (error) {
    fail('UI_ARTIFACT_IO', `failed to clean ${directory}: ${error.message}`);
  }
}

const pkgDir = path.resolve(required('pkg'));
const webDir = path.resolve(webDirFrom(import.meta.url));
const outDir = path.resolve(required('out'));
const runDirLabel = required('run-dir-label');
const scenarioPath = path.resolve(required('scenario'));
const mount = args.get('mount') ?? 'counter';
const isShow = mount === 'show';
const isKeyed = mount === 'keyed';
if (mount !== 'counter' && mount !== 'show' && mount !== 'keyed') {
  fail('UI_BROWSER_MOUNT', `unknown example mount ${mount}`);
}
const browserPath = args.get('browser');

cleanOutDir(outDir);

const scenario = fs.readFileSync(scenarioPath, 'utf8');
const scenarioDocument = JSON.parse(scenario);
const environment = scenarioDocument.environment;

const server = http.createServer((request, response) => {
  const pathname = new URL(request.url, 'http://127.0.0.1').pathname;
  let file;
  if (pathname === '/' || pathname === '/index.html') {
    file = path.join(webDir, 'index.html');
  } else {
    file = pkgFileFor(pkgDir, pathname);
  }
  if (!file) {
    response.writeHead(404);
    response.end('not found');
    return;
  }
  fs.readFile(file, (error, contents) => {
    if (error) {
      response.writeHead(404);
      response.end('not found');
      return;
    }
    const contentType = file.endsWith('.wasm')
      ? 'application/wasm'
      : file.endsWith('.js')
        ? 'text/javascript'
        : 'text/html';
    response.writeHead(200, { 'content-type': contentType });
    response.end(contents);
  });
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;

let browser;
try {
  try {
    browser = await chromium.launch({ headless: true, executablePath: browserPath || undefined });
  } catch (error) {
    error.code = 'UI_CHROMIUM_MISSING';
    throw error;
  }
  const context = await browser.newContext({
    viewport: { width: environment.viewport[0], height: environment.viewport[1] },
    deviceScaleFactor: environment.dpr,
    locale: environment.locale,
    colorScheme: environment.color_scheme,
  });
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (error) => pageErrors.push(error.message));

  // Counts `innerHTML` assignments so node reconstruction cannot pass unnoticed.
  // 统计 `innerHTML` 赋值，使节点重建无法蒙混过关。
  await page.addInitScript(() => {
    window.__finoInnerHtmlSets = 0;
    const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML');
    if (descriptor && descriptor.set) {
      Object.defineProperty(Element.prototype, 'innerHTML', {
        configurable: true,
        enumerable: descriptor.enumerable,
        get: descriptor.get,
        set(value) {
          window.__finoInnerHtmlSets += 1;
          descriptor.set.call(this, value);
        },
      });
    }
    window.__finoInsertBeforeCalls = 0;
    const insertBefore = Node.prototype.insertBefore;
    Node.prototype.insertBefore = function (child, reference) {
      window.__finoInsertBeforeCalls += 1;
      return insertBefore.call(this, child, reference);
    };
  });

  await page.goto(`${origin}/index.html`);
  await page.waitForFunction(() => window.fino instanceof Object);

  const initial = await page.evaluate(() => {
    const counter = new window.fino.CounterPage();
    const button = document.querySelector('button');
    const texts = Array.from(button.childNodes);
    const counts = { childList: 0, characterData: 0, values: [] };
    new MutationObserver((records) => {
      for (const record of records) {
        if (record.type === 'childList') {
          counts.childList += 1;
        }
        if (record.type === 'characterData') {
          counts.characterData += 1;
          counts.values.push(record.target.nodeValue);
        }
      }
    }).observe(button, { childList: true, subtree: true, characterData: true });
    window.__finoProbe = { counter, button, texts, counts };
    return {
      tag: button.tagName,
      text: button.textContent,
      texts: texts.map((node) => [node.nodeType, node.nodeValue]),
      inBody: button.parentNode === document.body,
      ariaLabel: button.getAttribute('aria-label'),
      innerHtmlSets: window.__finoInnerHtmlSets,
      viewport: [window.innerWidth, window.innerHeight],
      dpr: window.devicePixelRatio,
    };
  });

  // Reports the live DOM state without trusting the Rust snapshot alone.
  // 读取实时 DOM 状态，不只信任 Rust 快照。
  /**
   * Reads live DOM identity, mutation counts, and reactive counters.
   *
   * The button and both text nodes are compared by reference so a recreation
   * cannot satisfy a text-only assertion.
   * 读取实时 DOM 身份、变更计数与响应式计数。
   *
   * 按钮与两个文本节点按引用比较，因此重建节点无法满足仅检查文本的断言。
   *
   * @returns {Promise<object>} Snapshot of the probed page state.
   */
  const probe = () =>
    page.evaluate(() => {
      const { button, texts, counts, counter } = window.__finoProbe;
      const current = document.querySelector('button');
      const currentTexts = Array.from(current.childNodes);
      return {
        text: current.textContent,
        sameButton: current === button,
        sameTexts: currentTexts.length === texts.length && currentTexts.every((node, index) => node === texts[index]),
        childList: counts.childList,
        characterData: counts.characterData,
        values: counts.values.slice(),
        childOrder: currentTexts.map((node) => node.nodeValue),
        parentIsBody: current.parentNode === document.body,
        activeElementIsButton: document.activeElement === current,
        counters: JSON.parse(counter.execution_counters()),
        innerHtmlSets: window.__finoInnerHtmlSets,
      };
    });

  const button = page.getByRole('button', { name: 'Increment counter', exact: true });

  await runCheck('real_button_and_initial_text', async () => {
    assertEqual(initial.tag, 'BUTTON', 'element tag');
    assertEqual(initial.text, 'Count: 0', 'initial text');
    assert(initial.inBody, 'the counter must be mounted on document.body');
    assertEqual(initial.ariaLabel, 'Increment counter', 'aria-label');
    assertEqual(initial.texts.length, 2, 'text node count');
    assertEqual(initial.texts[0][1], 'Count: ', 'static text node');
    assertEqual(initial.texts[1][1], '0', 'dynamic text node');
    assertEqual(await button.count(), 1, 'accessible-name match count');
    assertEqual(initial.innerHtmlSets, 0, 'innerHTML assignments before interactions');
    assertEqual(initial.viewport[0], environment.viewport[0], 'browser viewport width');
    assertEqual(initial.viewport[1], environment.viewport[1], 'browser viewport height');
    assertEqual(initial.dpr, environment.dpr, 'browser device pixel ratio');
  });

  await runCheck('first_real_click_updates_node_value_once', async () => {
    await button.click();
    const state = await probe();
    assertEqual(state.text, 'Count: 1', 'text after first click');
    assertEqual(state.characterData, 1, 'characterData mutations after first click');
    assertEqual(state.childList, 0, 'childList mutations after first click');
    assertEqual(state.values.at(-1), '1', 'first nodeValue');
    assert(state.sameButton, 'the button node must keep its identity');
    assert(state.sameTexts, 'both text nodes must keep their identity');
    assertEqual(state.counters.component_executions, 1, 'component executions');
    assertEqual(state.counters.effect_executions, 2, 'effect executions');
    assertEqual(state.innerHtmlSets, 0, 'innerHTML assignments after first click');
  });

  await runCheck('second_real_click_updates_node_value_once', async () => {
    await button.click();
    const state = await probe();
    assertEqual(state.text, 'Count: 2', 'text after second click');
    assertEqual(state.characterData, 2, 'characterData mutations after second click');
    assertEqual(state.childList, 0, 'childList mutations after second click');
    assertEqual(state.values.at(-1), '2', 'second nodeValue');
    assert(state.sameButton && state.sameTexts, 'nodes keep their identity across clicks');
    assertEqual(state.counters.component_executions, 1, 'component executions');
    assertEqual(state.counters.effect_executions, 3, 'effect executions');
  });

  await runCheck('focus_and_keyboard_activation', async () => {
    await button.focus();
    let state = await probe();
    assert(state.activeElementIsButton, 'the real button must accept focus');

    await page.keyboard.press('Enter');
    state = await probe();
    assertEqual(state.text, 'Count: 3', 'text after Enter');
    assert(state.activeElementIsButton, 'Enter keeps focus on the button');
    assertEqual(state.characterData, 3, 'characterData mutations after Enter');
    assertEqual(state.childList, 0, 'childList mutations after Enter');
    assertEqual(state.counters.effect_executions, 4, 'effect executions after Enter');

    await page.keyboard.press('Space');
    state = await probe();
    assertEqual(state.text, 'Count: 4', 'text after Space');
    assertEqual(state.characterData, 4, 'characterData mutations after Space');
    assertEqual(state.counters.component_executions, 1, 'component executions');
    assertEqual(state.counters.effect_executions, 5, 'effect executions after Space');
    assertEqual(state.childOrder.slice(0, 1)[0], 'Count: ', 'DOM child order');
    assertEqual(state.innerHtmlSets, 0, 'innerHTML assignments after keyboard input');
  });

  await runCheck('batched_increments_commit_only_final_text', async () => {
    await page.evaluate(() => window.__finoProbe.counter.batch_increment(100));
    const state = await probe();
    assertEqual(state.text, 'Count: 104', 'text after batch');
    assertEqual(state.characterData, 5, 'characterData mutations after batch');
    assertEqual(state.values.at(-1), '104', 'batched nodeValue');
    assertEqual(state.childList, 0, 'childList mutations after batch');
    assert(state.sameButton && state.sameTexts, 'nodes keep their identity across the batch');
    assertEqual(state.counters.component_executions, 1, 'component executions');
    assertEqual(state.counters.effect_executions, 6, 'effect executions after batch');
  });

  await runCheck('tab_order_reaches_button', async () => {
    await page.goto(`${origin}/index.html`);
    await page.waitForFunction(() => window.fino instanceof Object);
    await page.evaluate(() => {
      window.__finoTabCounter = new window.fino.CounterPage();
    });
    await page.keyboard.press('Tab');
    const state = await page.evaluate(() => ({
      focused: document.activeElement === document.querySelector('button'),
      text: document.querySelector('button')?.textContent,
      inBody: document.querySelector('button')?.parentNode === document.body,
    }));
    assert(state.focused, 'Tab must reach the real button in DOM order');
    assertEqual(state.text, 'Count: 0', 'text after Tab focus');
    assert(state.inBody, 'Tab-focused counter lives on document.body');
  });

  // Reloads so the scenario mounts its own counter without sharing page state.
  // 重新加载页面，让场景挂载自己的计数器，不共享页面状态。
  await page.goto(`${origin}/index.html`);
  await page.waitForFunction(() => window.fino instanceof Object);

  let bundle;
  await runCheck('scenario_executes_in_chromium', async () => {
    const serialized = await page.evaluate(
      ({ scenario, runDirLabel, isShow, isKeyed }) =>
        isShow
          ? window.fino.run_show_scenario(scenario, runDirLabel)
          : isKeyed
            ? window.fino.run_keyed_scenario(scenario, runDirLabel)
            : window.fino.run_scenario(scenario, runDirLabel),
      { scenario, runDirLabel, isShow, isKeyed },
    );
    bundle = JSON.parse(serialized);
    assert(bundle.result, 'the scenario must return a result');
    assert(Array.isArray(bundle.result.steps), 'the scenario must return ordered steps');
    assertEqual(bundle.result.backend, 'web-headless', 'scenario backend');
    assert(
      bundle.result.steps.length >= 1 &&
        bundle.result.steps.length <= scenarioDocument.steps.length,
      'the scenario must report at least one executed step and no more than its protocol steps',
    );
    assertEqual(bundle.result.counters.component_executions, 1, 'scenario component executions');
  });

  await runCheck('scenario_dom_matches_live_dom', async () => {
    if (isShow) {
      const live = await page.evaluate(() => ({
        hostText: document.querySelector('button[aria-label="Show host"]')?.textContent,
        action: document.querySelector('button[aria-label="Show action"]'),
        innerHtmlSets: window.__finoInnerHtmlSets,
      }));
      const dom = JSON.parse(bundle.artifacts['dom.json']);
      assertEqual(dom.roots.length, 1, 'Show DOM root count');
      const anchors = dom.nodes.filter((node) => node.kind === 'anchor');
      assertEqual(anchors.length, 2, 'Show anchor count');
      const a11y = JSON.parse(bundle.artifacts['a11y.json']);
      const host = a11y.nodes.find((node) => node.name === 'Show host');
      assert(host, 'Show host must remain reachable');
      assertEqual(host.text, 'HostToggleTail', 'Show host text after hiding');
      assertEqual(live.hostText, 'HostToggleTail', 'live Show host text');
      assertEqual(live.action, null, 'removed Show action must be unreachable');
      assertEqual(live.innerHtmlSets, 0, 'innerHTML assignments during Show scenario');
      assertEqual(pageErrors.length, 0, 'Show page errors');
      return;
    }
    if (isKeyed) {
      const live = await page.evaluate(() => ({
        hostText: document.querySelector('button[aria-label="Keyed host"]')?.textContent,
        rows: Array.from(document.querySelectorAll('button[aria-label^="Row "]'))
          .map((node) => node.getAttribute('aria-label')),
        innerHtmlSets: window.__finoInnerHtmlSets,
      }));
      const dom = JSON.parse(bundle.artifacts['dom.json']);
      assertEqual(dom.roots.length, 1, 'keyed DOM root count');
      assertEqual(dom.nodes.filter((node) => node.kind === 'anchor').length, 6, 'keyed anchor count');
      const a11y = JSON.parse(bundle.artifacts['a11y.json']);
      const host = a11y.nodes.find((node) => node.name === 'Keyed host');
      assert(host, 'keyed host must remain reachable');
      assertEqual(host.text, 'HostNextD=1E=0Tail', 'keyed host text at capture');
      // Returning the scenario host drops its owner, so the keyed ranges release
      // while the static host remains in the live document.
      // 返回场景 Host 会释放其 Owner，因此 keyed 范围被释放，而静态宿主仍留在实时文档中。
      assertEqual(live.hostText, 'HostNextTail', 'live keyed host after host drop');
      assertEqual(live.rows.length, 0, 'keyed rows released after host drop');
      assertEqual(live.innerHtmlSets, 0, 'innerHTML assignments during keyed scenario');
      assertEqual(pageErrors.length, 0, 'keyed page errors');
      return;
    }
    const live = await page.evaluate(() => ({
      text: document.querySelector('button')?.textContent,
      inBody: document.querySelector('button')?.parentNode === document.body,
      innerHtmlSets: window.__finoInnerHtmlSets,
    }));
    const dom = JSON.parse(bundle.artifacts['dom.json']);
    assertEqual(dom.roots.length, 1, 'DOM root count');
    const domText = dom.nodes
      .filter((node) => node.kind === 'text')
      .map((node) => node.text)
      .join('');
    assertEqual(domText, live.text, 'snapshot text against live DOM text');
    assertEqual(dom.nodes[0].attributes['aria-label'], 'Increment counter', 'snapshot aria-label');
    const a11y = JSON.parse(bundle.artifacts['a11y.json']);
    assertEqual(a11y.nodes.length, 1, 'accessibility node count');
    assertEqual(a11y.nodes[0].name, 'Increment counter', 'accessibility name');
    assertEqual(a11y.nodes[0].text, live.text, 'accessibility text against live DOM text');
    assert(live.inBody, 'scenario counter lives on document.body');
    assertEqual(live.innerHtmlSets, 0, 'innerHTML assignments during scenario');
    assertEqual(pageErrors.length, 0, 'page errors');
  });

  // Reloads so the conformance sequence mounts into a clean body.
  // 重新加载，使一致性序列挂载到干净的 body。
  await page.goto(`${origin}/index.html`);
  await page.waitForFunction(() => window.fino instanceof Object);

  let conformance;
  let detachReinsert;
  let remount;
  let anchorMisuse;
  let showConformance;
  let keyedConformance;
  await runCheck('conformance_reports_shared_runtime_stats', async () => {
    const serialized = await page.evaluate(() => window.fino.run_conformance());
    conformance = JSON.parse(serialized);
    assertEqual(conformance.schema, 1, 'conformance schema');
    assertEqual(conformance.backend, 'web-headless', 'conformance backend');
    assertEqual(conformance.status, 'passed', 'conformance status');
    assertEqual(conformance.violations.length, 0, 'conformance violations');
    assertEqual(conformance.checkpoints.mounted.component_executions, 1, 'mounted component executions');
    assertEqual(conformance.checkpoints.mounted.effect_executions, 1, 'mounted effect executions');
    assertEqual(conformance.checkpoints.mounted.backend_set_text, 1, 'mounted text commits');
    assertEqual(conformance.checkpoints.mounted.nodes_created, 3, 'mounted node creations');
    assertEqual(conformance.checkpoints.text_changed.backend_set_text, 2, 'text change commits');
    assertEqual(conformance.checkpoints.clicked.backend_set_text, 3, 'click text commits');
    assertEqual(conformance.event.text_after, 'Count: !2', 'clicked text');
    assert(conformance.event.handled, 'click must be handled');
    assertEqual(conformance.event.selector_matches, 1, 'clicked selector matches');
    assertEqual(conformance.batch.final_text, 'Count: !102', 'batched text');
    assertEqual(conformance.batch.set_text_delta, 1, 'batch text commits');
    assertEqual(conformance.batch.nodes_created_delta, 0, 'batch node creations');
    assertEqual(conformance.batch.nodes_removed_delta, 0, 'batch node removals');
    assertEqual(conformance.batch.frames_requested_delta, 0, 'batch frame requests');
    assertEqual(conformance.unmount.selector_matches, 0, 'unmounted selector matches');
    assertEqual(conformance.unmount.a11y_nodes, 0, 'unmounted a11y nodes');
    assertEqual(conformance.unmount.nodes_removed_delta, 1, 'unmount removals');
    assertEqual(conformance.final_tree.roots.length, 0, 'unmounted logical tree');
    assertEqual(conformance.operations.create_element, 1, 'create_element operations');
    assertEqual(conformance.operations.create_text, 3, 'create_text operations');
    assertEqual(conformance.operations.insert, 4, 'insert operations');
    assertEqual(conformance.operations.set_attribute, 3, 'set_attribute operations');
    assertEqual(conformance.operations.listen, 1, 'listen operations');
    assertEqual(conformance.operations.set_text, 4, 'set_text operations');
    assertEqual(conformance.operations.unlisten, 1, 'unlisten operations');
    assertEqual(conformance.operations.remove, 1, 'remove operations');
  });

  await runCheck('detach_reinsert_matches_shared_semantics', async () => {
    const serialized = await page.evaluate(() => window.fino.run_detach_reinsert_probe());
    detachReinsert = JSON.parse(serialized);
    assertEqual(detachReinsert.schema, 1, 'detach probe schema');
    assertEqual(detachReinsert.backend, 'web-headless', 'detach probe backend');
    assertEqual(detachReinsert.text_detached, 'Count: ', 'detached text');
    assertEqual(detachReinsert.text_restored, 'Count: 0', 'restored text');
    assertEqual(detachReinsert.tree_detached.roots[0].children.length, 1, 'detached tree children');
    assertEqual(detachReinsert.tree_restored.roots[0].children.length, 2, 'restored tree children');
    assertEqual(detachReinsert.detached_stats.nodes_removed, 1, 'detached removals');
    assertEqual(detachReinsert.operations.insert, 3, 'detach probe insert operations');
    assertEqual(detachReinsert.operations.remove, 1, 'detach probe remove operations');
  });

  await runCheck('show_ranges_match_shared_semantics', async () => {
    const serialized = await page.evaluate(() => window.fino.run_show_conformance_probe());
    showConformance = JSON.parse(serialized);
    assertEqual(showConformance.schema, 1, 'Show conformance schema');
    assertEqual(showConformance.backend, 'web-headless', 'Show conformance backend');
    assertEqual(showConformance.status, 'passed', 'Show conformance status');
    assertEqual(showConformance.violations.length, 0, 'Show conformance violations');
    assertEqual(showConformance.initial_tree.roots.length, 1, 'Show initial roots');
    assertEqual(showConformance.hidden_tree.roots.length, 1, 'Show hidden roots');
    assertEqual(showConformance.final_tree.roots.length, 0, 'Show final roots');
    assertEqual(showConformance.checkpoints.hidden.action_matches, 0, 'hidden action matches');
    assertEqual(showConformance.checkpoints.restored.action_matches, 1, 'restored action matches');
    assertEqual(showConformance.checkpoints.hidden.listeners, 1, 'hidden listeners');
    assertEqual(showConformance.checkpoints.restored.listeners, 2, 'restored listeners');
    assert(showConformance.static_ids_stable, 'static Show ids stay stable');
    assert(showConformance.stale_write_is_noop, 'removed dependencies do not reach the DOM');
    assertEqual(showConformance.operations.create_anchor, 2, 'Show create_anchor operations');
    assert(showConformance.storage_stable, 'Show storage does not grow across toggles');
    assertEqual(showConformance.storage_loops, 10, 'Show storage loop count');
    assertEqual(showConformance.operations.remove, 1, 'Show remove operations');
    assertEqual(showConformance.operations.release, 26, 'Show release operations');
    assertEqual(showConformance.operations.unlisten, 13, 'Show unlisten operations');
  });

  await runCheck('keyed_for_matches_shared_semantics', async () => {
    const serialized = await page.evaluate(() => window.fino.run_keyed_conformance_probe());
    keyedConformance = JSON.parse(serialized);
    assertEqual(keyedConformance.schema, 1, 'keyed conformance schema');
    assertEqual(keyedConformance.backend, 'web-headless', 'keyed conformance backend');
    assertEqual(keyedConformance.status, 'passed', 'keyed conformance status');
    assertEqual(keyedConformance.violations.length, 0, 'keyed conformance violations');
    assertEqual(keyedConformance.initial_text, 'HostNextTail', 'keyed initial text');
    assertEqual(keyedConformance.phases.length, 11, 'keyed phase count');
    assert(keyedConformance.item_identity_stable, 'keyed item identity stays stable');
    assert(keyedConformance.probe_storage_bounded, 'keyed probe storage stays bounded');
    assertEqual(Object.keys(keyedConformance.builder_runs).length, 8, 'cumulative builder runs survive disposal');
    assert(keyedConformance.static_ids_stable, 'keyed static ids stay stable');
    assert(keyedConformance.stale_write_is_noop, 'removed key writes stay local');
    assert(keyedConformance.duplicate_key_rejected, 'duplicate keys reject atomically');
    assert(keyedConformance.duplicate_recovered, 'valid key update recovers');
    assert(keyedConformance.dispose_idempotent, 'keyed disposal is idempotent');
    assertEqual(keyedConformance.operations.create_element, 10, 'keyed create_element');
    assertEqual(keyedConformance.operations.create_text, 11, 'keyed create_text');
    assertEqual(keyedConformance.operations.create_anchor, 18, 'keyed create_anchor');
    assertEqual(keyedConformance.operations.insert, 59, 'keyed insert moves');
    assertEqual(keyedConformance.operations.release, 34, 'keyed releases');
    assertEqual(keyedConformance.operations.remove, 1, 'keyed host remove');
    assertEqual(keyedConformance.local_update.reconcile_delta, 0, 'item updates skip reconciliation');
  });

  await runCheck('keyed_rows_keep_dom_identity_and_insert_before_moves', async () => {
    const state = await page.evaluate(() => {
      const keyed = new window.fino.KeyedPage();
      window.__finoKeyedPage = keyed;
      keyed.set_keys(JSON.stringify(['A', 'B', 'C']));
      const capture = () => {
        const rows = {};
        for (const key of ['A', 'B', 'C', 'X']) {
          const button = document.querySelector(`button[aria-label="Row ${key}"]`);
          rows[key] = button ? { button, text: button.firstChild } : null;
        }
        return rows;
      };
      window.__finoKeyedRows = capture();
      window.__finoKeyedInnerHtml = window.__finoInnerHtmlSets;
      return {
        keys: JSON.parse(keyed.keys()),
        runs: JSON.parse(keyed.builder_runs()),
        innerHtmlSets: window.__finoInnerHtmlSets,
      };
    });
    assertEqual(state.keys.join(','), 'A,B,C', 'keyed initial keys');
    assertEqual(state.innerHtmlSets, 0, 'keyed initial innerHTML assignments');

    const prepended = await page.evaluate(() => {
      const keyed = window.__finoKeyedPage;
      const before = window.__finoKeyedRows;
      keyed.set_keys(JSON.stringify(['X', 'A', 'B', 'C']));
      const rows = {};
      for (const key of ['A', 'B', 'C']) {
        const button = document.querySelector(`button[aria-label="Row ${key}"]`);
        rows[key] = { button, text: button?.firstChild };
      }
      window.__finoKeyedRows = { ...before, X: { button: document.querySelector('button[aria-label="Row X"]'), text: document.querySelector('button[aria-label="Row X"]')?.firstChild } };
      return {
        same: ['A', 'B', 'C'].every(
          (key) => rows[key].button === before[key].button && rows[key].text === before[key].text,
        ),
        order: Array.from(document.querySelectorAll('button[aria-label^="Row "]')).map((node) =>
          node.getAttribute('aria-label'),
        ),
        runs: JSON.parse(keyed.builder_runs()),
      };
    });
    assert(prepended.same, 'prepend keeps A/B/C DOM identities');
    assertEqual(prepended.order.join(','), 'Row X,Row A,Row B,Row C', 'prepend order');
    assertEqual(prepended.runs.A, 1, 'prepend never rebuilds A');
    assertEqual(prepended.runs.B, 1, 'prepend never rebuilds B');
    assertEqual(prepended.runs.C, 1, 'prepend never rebuilds C');

    const reordered = await page.evaluate(() => {
      const keyed = window.__finoKeyedPage;
      const before = window.__finoKeyedRows;
      const insertBeforeBefore = window.__finoInsertBeforeCalls;
      keyed.set_keys(JSON.stringify(['C', 'B', 'A', 'X']));
      const after = {};
      for (const key of ['A', 'B', 'C', 'X']) {
        const button = document.querySelector(`button[aria-label="Row ${key}"]`);
        after[key] = { button, text: button?.firstChild };
      }
      window.__finoKeyedRows = after;
      return {
        same: ['A', 'B', 'C', 'X'].every(
          (key) => after[key].button === before[key].button && after[key].text === before[key].text,
        ),
        order: Array.from(document.querySelectorAll('button[aria-label^="Row "]')).map((node) =>
          node.getAttribute('aria-label'),
        ),
        insertBeforeDelta: window.__finoInsertBeforeCalls - insertBeforeBefore,
        innerHtmlSets: window.__finoInnerHtmlSets,
        keys: JSON.parse(keyed.keys()),
      };
    });
    assert(reordered.same, 'reverse keeps every row and text node identity');
    assertEqual(reordered.order.join(','), 'Row C,Row B,Row A,Row X', 'reversed DOM order');
    assertEqual(reordered.insertBeforeDelta, 9, 'four-item reverse keeps one stable range and moves three complete ranges');
    assertEqual(reordered.innerHtmlSets, 0, 'reverse never assigns innerHTML');

    const local = await page.evaluate(() => {
      const keyed = window.__finoKeyedPage;
      const before = window.__finoKeyedRows.B;
      document.querySelector('button[aria-label="Row B"]').click();
      const after = document.querySelector('button[aria-label="Row B"]');
      return {
        text: after.textContent,
        sameButton: after === before.button,
        sameText: after.firstChild === before.text,
        runs: JSON.parse(keyed.builder_runs()),
      };
    });
    assertEqual(local.text, 'B=1', 'item-local click text');
    assert(local.sameButton && local.sameText, 'item-local click keeps DOM identity');
    assertEqual(local.runs.B, 1, 'item-local click never rebuilds B');

    const bounded = await page.evaluate(() => {
      const keyed = window.__finoKeyedPage;
      for (let index = 0; index < 25; index += 1) {
        keyed.set_keys(JSON.stringify([`K${index}`]));
      }
      return {
        probes: JSON.parse(keyed.probe_keys()),
        keys: JSON.parse(keyed.keys()),
        runs: JSON.parse(keyed.builder_runs()),
      };
    });
    assertEqual(bounded.probes.length, 1, 'probe storage keeps only live keys');
    assertEqual(bounded.probes[0], 'K24', 'probe storage keeps the newest key');
    assertEqual(bounded.keys[0], 'K24', 'churn commits the newest key');
    assertEqual(Object.keys(bounded.runs).length, 1, 'builder runs stay bounded');

    const cleared = await page.evaluate(() => {
      const keyed = window.__finoKeyedPage;
      keyed.set_keys(JSON.stringify([]));
      const empty = document.querySelectorAll('button[aria-label^="Row "]').length;
      keyed.dispose();
      return {
        empty,
        innerHtmlSets: window.__finoInnerHtmlSets,
        pageErrors: 0,
      };
    });
    assertEqual(cleared.empty, 0, 'clearing releases every row');
    assertEqual(cleared.innerHtmlSets, 0, 'clearing never assigns innerHTML');
    assertEqual(pageErrors.length, 0, 'keyed identity page errors');
  });

  await runCheck('anchor_nodes_reject_text_writes', async () => {
    const serialized = await page.evaluate(() => window.fino.run_anchor_misuse_probe());
    anchorMisuse = JSON.parse(serialized);
    assertEqual(anchorMisuse.schema, 1, 'anchor misuse probe schema');
    assertEqual(anchorMisuse.anchor_accepts_text, false, 'anchors must reject set_text');
    assertEqual(anchorMisuse.text_accepts_text, true, 'text nodes must accept set_text');
  });

  await runCheck('web_remount_restores_reachability', async () => {
    const serialized = await page.evaluate(() => window.fino.run_remount_probe());
    remount = JSON.parse(serialized);
    assertEqual(remount.schema, 1, 'remount probe schema');
    assertEqual(remount.mounted_matches, 1, 'mounted matches');
    assertEqual(remount.removed_matches, 0, 'removed matches');
    assertEqual(remount.removed_tree_roots, 0, 'removed tree roots');
    assertEqual(remount.removed_a11y_nodes, 0, 'removed a11y nodes');
    assertEqual(remount.remounted_matches, 1, 'remounted matches');
    assertEqual(remount.remounted_tree_roots, 1, 'remounted tree roots');
    assertEqual(remount.remounted_a11y_nodes, 1, 'remounted a11y nodes');
  });

  // Writes the exact artifact bytes the wasm run produced.
  // 写入 wasm 运行产生的精确产物字节。
  for (const [relative, contents] of Object.entries(bundle.artifacts)) {
    const destination = path.join(outDir, relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, contents);
  }
  fs.writeFileSync(
    path.join(outDir, 'conformance.json'),
    `${JSON.stringify(conformance, null, 2)}\n`,
  );
  fs.writeFileSync(
    path.join(outDir, 'detach_reinsert.json'),
    `${JSON.stringify(detachReinsert, null, 2)}\n`,
  );
  fs.writeFileSync(
    path.join(outDir, 'remount.json'),
    `${JSON.stringify(remount, null, 2)}\n`,
  );
  fs.writeFileSync(
    path.join(outDir, 'anchor_misuse.json'),
    `${JSON.stringify(anchorMisuse, null, 2)}\n`,
  );
  fs.writeFileSync(
    path.join(outDir, 'show_conformance.json'),
    `${JSON.stringify(showConformance, null, 2)}\n`,
  );
  fs.writeFileSync(
    path.join(outDir, 'keyed_conformance.json'),
    `${JSON.stringify(keyedConformance, null, 2)}\n`,
  );
  fs.writeFileSync(
    path.join(outDir, 'browser.json'),
    `${JSON.stringify({ schema: 1, backend: 'web-headless', status: 'passed', checks }, null, 2)}\n`,
  );

  process.stdout.write(
    `${JSON.stringify({
      status: 'passed',
      result: bundle.result,
      checks,
      conformance,
      artifacts: [
        ...Object.keys(bundle.artifacts),
        'conformance.json',
        'detach_reinsert.json',
        'remount.json',
        'anchor_misuse.json',
        'show_conformance.json',
        'keyed_conformance.json',
      ],
    })}\n`,
  );
} catch (error) {
  const code = error.code?.startsWith?.('UI_') ? error.code : 'UI_BROWSER_ASSERTION';
  emitFailure(code, error.message);
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
