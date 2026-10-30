// Headless Chromium driver for the counter's wasm build.
// 计数器 wasm 构建的无头 Chromium 驱动。

import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

let chromium;
const checks = [];
try {
  ({ chromium } = await import('playwright'));
} catch (error) {
  fail('UI_PLAYWRIGHT_MISSING', `playwright is not installed; run npm ci --prefix examples/counter/web (${error.message})`);
}

const args = parseArgs(process.argv.slice(2));

// Reads one required driver argument.
// 读取一个必需的驱动参数。
function required(name) {
  const value = args.get(name);
  if (!value) {
    fail('UI_BROWSER_ARGUMENT', `missing required --${name} argument`);
  }
  return value;
}

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

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

async function runCheck(name, operation) {
  try {
    await operation();
    checks.push({ name, status: 'passed' });
  } catch (error) {
    checks.push({ name, status: 'failed', message: error.message });
    throw error;
  }
}

// Reports a structured failure without pretending the run succeeded.
// 报告结构化失败，绝不伪装成成功。
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

function fail(code, message, report = undefined) {
  emitFailure(code, message, report);
  process.exit(1);
}

const pkgDir = path.resolve(required('pkg'));
const webDir = path.resolve(path.dirname(new URL(import.meta.url).pathname));
const outDir = path.resolve(required('out'));
const runDirLabel = required('run-dir-label');
const scenarioPath = path.resolve(required('scenario'));
const browserPath = args.get('browser');
const scenario = fs.readFileSync(scenarioPath, 'utf8');
const scenarioDocument = JSON.parse(scenario);
const environment = scenarioDocument.environment;

const server = http.createServer((request, response) => {
  const pathname = decodeURIComponent(new URL(request.url, 'http://127.0.0.1').pathname);
  let file;
  if (pathname === '/' || pathname === '/index.html') {
    file = path.join(webDir, 'index.html');
  } else if (pathname.startsWith('/pkg/')) {
    file = path.join(pkgDir, pathname.slice('/pkg/'.length));
  } else {
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
      ({ scenario, runDirLabel }) => window.fino.run_scenario(scenario, runDirLabel),
      { scenario, runDirLabel },
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

  // Writes the exact artifact bytes the wasm run produced.
  // 写入 wasm 运行产生的精确产物字节。
  for (const [relative, contents] of Object.entries(bundle.artifacts)) {
    const destination = path.join(outDir, relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, contents);
  }
  fs.writeFileSync(
    path.join(outDir, 'browser.json'),
    `${JSON.stringify({ schema: 1, backend: 'web-headless', status: 'passed', checks }, null, 2)}\n`,
  );

  process.stdout.write(
    `${JSON.stringify({
      status: 'passed',
      result: bundle.result,
      checks,
      artifacts: Object.keys(bundle.artifacts),
    })}\n`,
  );
} catch (error) {
  const code = error.code?.startsWith?.('UI_') ? error.code : 'UI_BROWSER_ASSERTION';
  emitFailure(code, error.message);
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
