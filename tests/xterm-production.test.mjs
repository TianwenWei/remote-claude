import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import test from 'node:test';
import { JSDOM } from 'jsdom';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** Run the actual production terminal factory without importing the app's
 * bootstrap. The only skipped code is Vite's final namespace interop; no
 * parser code or missing variable declaration is replaced or injected. */
function productionTerminal(dom) {
  const assets = path.join(root, 'dist/assets');
  const file = process.env.XTERM_TEST_BUNDLE ?? path.join(assets, readdirSync(assets).find((name) => /^xterm-.*\.js$/.test(name)) ?? 'missing-xterm.js');
  const source = readFileSync(file, 'utf8');
  const imports = source.match(/^(?:import[^;]+;\s*)*/)?.[0] ?? '';
  const cjs = /var ([$\w]+)=([$\w]+)\(\);const [$\w]+=[$\w]+\(\1\),[^;]+;export\{[^}]+\};\s*$/.exec(source);
  let code;
  if (cjs) {
    code = source.slice(imports.length, cjs.index) + `globalThis.terminalExports=${cjs[2]}();`;
  } else {
    const exported = /export\{([$\w]+) as (?:Terminal|x)\};?\s*$/.exec(source);
    assert.ok(exported, 'Unrecognized production xterm export; update the test harness for the new bundler output');
    code = source.slice(imports.length, exported.index) + `globalThis.terminalExports=${exported[1]};`;
  }
  const context = vm.createContext({
    self: dom.window, window: dom.window, document: dom.window.document,
    navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement,
    setTimeout, clearTimeout, setInterval, clearInterval, queueMicrotask,
    performance, console,
  });
  vm.runInContext(code, context, { filename: file });
  const exported = context.terminalExports;
  const Terminal = exported.Terminal ?? exported;
  assert.equal(typeof Terminal, 'function');
  return Terminal;
}

function write(terminal, input) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Terminal write did not complete')), 2000);
    terminal.write(input, () => { clearTimeout(timer); resolve(); });
  });
}

test('production xterm answers ANSI/DEC mode queries and keeps parsing subsequent input', { timeout: 10000 }, async () => {
  const dom = new JSDOM('<!doctype html><html><body></body></html>', { pretendToBeVisual: true });
  // This parser test never opens a renderer. Model an unavailable canvas
  // context rather than asking jsdom to provide its unimplemented graphics.
  dom.window.HTMLCanvasElement.prototype.getContext = () => null;
  const Terminal = productionTerminal(dom);
  const terminal = new Terminal();
  const replies = [];
  const subscription = terminal.onData((reply) => replies.push(reply));
  const query = async (input, expected) => {
    const before = replies.length;
    await write(terminal, input);
    assert.deepEqual(replies.slice(before), [expected]);
  };
  try {
    await query('\x1b[?2026$p', '\x1b[?2026;2$y');
    await query('\x1b[?2026h\x1b[?2026$p', '\x1b[?2026;1$y');
    await query('\x1b[?2026l\x1b[?2026$p', '\x1b[?2026;2$y');
    await query('\x1b[4$p', '\x1b[4;2$y');
    await query('\x1b[4h\x1b[4$p', '\x1b[4;1$y');
    await query('\x1b[4l\x1b[4$p', '\x1b[4;2$y');
    await query('\x1b[2$p', '\x1b[2;4$y');
    await query('\x1b[?9999$p', '\x1b[?9999;0$y');
    await query('\x1b[9999$p', '\x1b[9999;0$y');
    const before = replies.length;
    await write(terminal, '\x1b[?2026');
    await write(terminal, '$');
    assert.equal(replies.length, before);
    await query('p', '\x1b[?2026;2$y');
    await write(terminal, 'after-mode-query');
    assert.equal(terminal.buffer.active.getLine(0).translateToString(true), 'after-mode-query');
  } finally {
    subscription.dispose();
    terminal.dispose();
    dom.window.close();
  }
});
