// One browser tab driven through CDP, with DOM helpers and console/network collectors.
//
// Every scenario asserts on what the page really displays (`document...` evaluated in
// the tab), not on an HTTP response. The same object records uncaught exceptions,
// console errors, log errors, failed requests and 4xx/5xx responses so a scenario can
// state which of them were expected (and which were not).

import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { sleep } from './server.mjs';

const DEFAULT_TIMEOUT_MS = 15_000;
const DEFAULT_INTERVAL_MS = 100;

export class PageEvaluationError extends Error {
  constructor(label, description) {
    super(`évaluation dans la page "${label}" échouée: ${description}`);
    this.name = 'PageEvaluationError';
  }
}

export class Page {
  #connection;
  #sessionId;
  #unsubscribers = [];
  #closed = false;

  constructor({ connection, targetId, sessionId, label = 'page' }) {
    this.#connection = connection;
    this.#sessionId = sessionId;
    this.targetId = targetId;
    this.label = label;
    this.exceptions = [];
    this.consoleMessages = [];
    this.consoleErrors = [];
    this.logErrors = [];
    this.httpErrors = [];
    this.networkFailures = [];
    this.requests = new Map();
    this.#bindCollectors();
  }

  static async create({ connection, targetId, sessionId, label, url }) {
    const page = new Page({ connection, targetId, sessionId, label });
    await page.#initialize();
    if (url) await page.navigate(url);
    return page;
  }

  #bindCollectors() {
    const bind = (method, handler) => {
      const unsubscribe = this.#connection.on(method, (params, message) => {
        if (message.sessionId !== this.#sessionId) return;
        handler(params);
      });
      this.#unsubscribers.push(unsubscribe);
    };

    bind('Runtime.exceptionThrown', (params) => {
      const details = params.exceptionDetails ?? {};
      this.exceptions.push({
        text: details.text ?? '',
        description: details.exception?.description ?? null,
        url: details.url ?? null,
        lineNumber: details.lineNumber ?? null,
        columnNumber: details.columnNumber ?? null,
      });
    });
    bind('Runtime.consoleAPICalled', (params) => {
      const text = (params.args ?? [])
        .map((arg) => arg.value ?? arg.description ?? arg.unserializableValue ?? '')
        .join(' ');
      const entry = { type: params.type ?? 'log', text, timestamp: params.timestamp ?? null };
      this.consoleMessages.push(entry);
      if (entry.type === 'error' || entry.type === 'assert') {
        this.consoleErrors = this.consoleErrors ?? [];
        this.consoleErrors.push(entry);
      }
    });
    bind('Log.entryAdded', (params) => {
      const entry = params.entry ?? {};
      if (entry.level === 'error') {
        this.logErrors.push({
          source: entry.source ?? null,
          text: entry.text ?? '',
          url: entry.url ?? null,
        });
      }
      this.consoleMessages.push({
        type: `log:${entry.level ?? 'unknown'}`,
        text: entry.text ?? '',
        timestamp: entry.timestamp ?? null,
      });
    });
    bind('Network.requestWillBeSent', (params) => {
      this.requests.set(params.requestId, {
        url: params.request?.url ?? '',
        method: params.request?.method ?? '',
      });
    });
    bind('Network.responseReceived', (params) => {
      const response = params.response ?? {};
      const request = this.requests.get(params.requestId);
      if (request) request.responseStatus = response.status ?? null;
      if (!/^https?:/i.test(response.url ?? '')) return;
      if ((response.status ?? 0) >= 400) {
        this.httpErrors.push({
          status: response.status,
          url: response.url,
          resourceType: params.type ?? null,
          method: this.requests.get(params.requestId)?.method ?? null,
        });
      }
    });
    bind('Network.loadingFailed', (params) => {
      const request = this.requests.get(params.requestId);
      const url = request?.url ?? '';
      if (!/^https?:/i.test(url)) return;
      this.networkFailures.push({
        url,
        method: request?.method ?? null,
        responseStatus: request?.responseStatus ?? null,
        errorText: params.errorText ?? '',
        canceled: Boolean(params.canceled),
        blockedReason: params.blockedReason ?? null,
        corsErrorStatus: params.corsErrorStatus ?? null,
      });
    });
  }

  async #initialize() {
    const send = (method, params = {}) =>
      this.#connection.send(method, params, { sessionId: this.#sessionId });
    await send('Runtime.enable');
    await send('Page.enable');
    await send('Log.enable');
    await send('Network.enable');
  }

  get closed() {
    return this.#closed;
  }

  /** Evaluate a JS expression in the tab and return its JSON value. */
  async evaluate(expression, { awaitPromise = false, timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    const result = await this.#connection.send(
      'Runtime.evaluate',
      { expression, returnByValue: true, awaitPromise, userGesture: false },
      { sessionId: this.#sessionId, timeoutMs },
    );
    if (result.exceptionDetails) {
      const details = result.exceptionDetails;
      throw new PageEvaluationError(
        this.label,
        details.exception?.description ?? details.text ?? 'exception sans description',
      );
    }
    if (result.result?.subtype === 'error') {
      throw new PageEvaluationError(this.label, result.result.description ?? 'objet Error');
    }
    return result.result?.value;
  }

  /** Poll an expression until it is truthy, keeping the last evaluation error. */
  async waitFor(expression, label, { timeoutMs = DEFAULT_TIMEOUT_MS, intervalMs = DEFAULT_INTERVAL_MS } = {}) {
    const deadline = Date.now() + timeoutMs;
    let lastError = null;
    let lastValue;
    while (Date.now() < deadline) {
      try {
        lastValue = await this.evaluate(expression);
        if (lastValue) return lastValue;
      } catch (error) {
        lastError = error;
      }
      await sleep(intervalMs);
    }
    throw new Error(
      `délai dépassé (${timeoutMs} ms) en attendant ${label} dans la page "${this.label}"` +
        (lastError ? `; dernière erreur: ${lastError.message}` : `; dernière valeur: ${JSON.stringify(lastValue)}`),
    );
  }

  async navigate(url) {
    const expected = new URL(url).toString();
    await this.#connection.send(
      'Page.navigate',
      { url: expected },
      { sessionId: this.#sessionId, timeoutMs: 30_000 },
    );
    await this.waitFor(
      `location.href === ${JSON.stringify(expected)} && document.readyState === 'complete'`,
      `le chargement de ${expected}`,
      { timeoutMs: 30_000 },
    );
  }

  async waitForBanner({ compatible = true, timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    const expression = `(() => {
      const banner = document.querySelector('#banner');
      if (!banner || getComputedStyle(banner).display === 'none') return false;
      const text = banner.textContent || '';
      const matches = ${compatible ? `/Contrat v\\d+ vérifié/.test(text)` : `banner.classList.contains('warn')`};
      return matches ? text : false;
    })()`;
    return await this.waitFor(
      expression,
      compatible ? "le bandeau « contrat vérifié »" : "le bandeau d'incompatibilité de contrat",
      { timeoutMs },
    );
  }

  async waitForElement(selector, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    return await this.waitFor(
      `Boolean(document.querySelector(${JSON.stringify(selector)}))`,
      `l'élément ${selector}`,
      { timeoutMs },
    );
  }

  async waitForText(selector, expected, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    return await this.waitFor(
      `(() => { const el = document.querySelector(${JSON.stringify(selector)});
                return el && el.textContent.includes(${JSON.stringify(expected)}) ? el.textContent : false; })()`,
      `${selector} contenant ${JSON.stringify(expected)}`,
      { timeoutMs },
    );
  }

  async text(selector) {
    return await this.evaluate(
      `(() => { const el = document.querySelector(${JSON.stringify(selector)});
                return el ? el.textContent : null; })()`,
    );
  }

  async count(selector) {
    return await this.evaluate(`document.querySelectorAll(${JSON.stringify(selector)}).length`);
  }

  async titles() {
    return await this.evaluate(
      `[...document.querySelectorAll('#tasks li .title')].map((el) => el.textContent)`,
    );
  }

  async listItems() {
    return await this.evaluate(`[...document.querySelectorAll('#tasks li')].map((item) => ({
      title: item.querySelector('.title')?.textContent ?? null,
      due: item.querySelector('.due')?.textContent ?? null,
      done: item.classList.contains('done'),
      buttons: [...item.querySelectorAll('button')].map((b) => b.textContent.trim()),
    }))`);
  }

  async errorText() {
    return (await this.text('#error')) ?? '';
  }

  async waitForTask(title, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    return await this.waitFor(
      `[...document.querySelectorAll('#tasks li .title')].some((el) => el.textContent === ${JSON.stringify(title)})`,
      `la tâche ${JSON.stringify(title)} dans le DOM`,
      { timeoutMs },
    );
  }

  async waitForNoTask(title, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    return await this.waitFor(
      `![...document.querySelectorAll('#tasks li .title')].some((el) => el.textContent === ${JSON.stringify(title)})`,
      `la disparition de ${JSON.stringify(title)} du DOM`,
      { timeoutMs },
    );
  }

  async waitForErrorContaining(fragment, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    return await this.waitFor(
      `(() => { const el = document.querySelector('#error');
                return el && el.textContent.includes(${JSON.stringify(fragment)}) ? el.textContent : false; })()`,
      `un message d'erreur contenant ${JSON.stringify(fragment)}`,
      { timeoutMs },
    );
  }

  /** Set an input value the way a user would (native setter + input/change events). */
  async fill(selector, value) {
    const result = await this.evaluate(`(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) throw new Error('élément introuvable: ' + ${JSON.stringify(selector)});
      const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
      setter.call(el, ${JSON.stringify(value)});
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return el.value;
    })()`);
    return result;
  }

  async click(selector) {
    await this.evaluate(`(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) throw new Error('élément introuvable: ' + ${JSON.stringify(selector)});
      el.click();
      return true;
    })()`);
  }

  /** Click the first button whose trimmed text equals `text` inside `scopeSelector`. */
  async clickButtonWithText(scopeSelector, text) {
    return await this.evaluate(`(() => {
      const scope = document.querySelector(${JSON.stringify(scopeSelector)});
      if (!scope) throw new Error('conteneur introuvable: ' + ${JSON.stringify(scopeSelector)});
      const button = [...scope.querySelectorAll('button')]
        .find((candidate) => candidate.textContent.trim() === ${JSON.stringify(text)});
      if (!button) throw new Error('bouton introuvable: ' + ${JSON.stringify(text)});
      button.click();
      return true;
    })()`);
  }

  async isDisabled(selector) {
    return await this.evaluate(`(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      return el ? Boolean(el.disabled) : null;
    })()`);
  }

  async screenshot(filePath) {
    const result = await this.#connection.send(
      'Page.captureScreenshot',
      { format: 'png', fromSurface: true },
      { sessionId: this.#sessionId, timeoutMs: 20_000 },
    );
    await writeFile(filePath, Buffer.from(result.data, 'base64'));
    return path.resolve(filePath);
  }

  /** Snapshot of everything observed in this tab so far. */
  snapshot() {
    return {
      label: this.label,
      exceptions: [...this.exceptions],
      consoleErrors: [...(this.consoleErrors ?? [])],
      consoleMessages: [...this.consoleMessages],
      logErrors: [...this.logErrors],
      httpErrors: [...this.httpErrors],
      networkFailures: [...this.networkFailures],
      requestCount: this.requests.size,
    };
  }

  /**
   * Assert that nothing unexpected appeared in the console/network. Every category
   * accepts RegExp allow-lists (the scenario documents why each entry is expected).
   */
  assertCleanConsole({
    allowExceptions = [],
    allowConsoleErrors = [],
    allowLogErrors = [],
    allowHttpErrors = [],
    allowNetworkFailures = [],
  } = {}) {
    const matches = (value, patterns) => patterns.some((pattern) => pattern.test(value));
    const unexpected = (entries, patterns, describe) =>
      entries.filter((entry) => !matches(describe(entry), patterns));

    const unexpectedExceptions = unexpected(
      this.exceptions,
      allowExceptions,
      (entry) => `${entry.text} ${entry.description ?? ''}`,
    );
    const unexpectedConsoleErrors = unexpected(
      this.consoleErrors ?? [],
      allowConsoleErrors,
      (entry) => entry.text ?? '',
    );
    const unexpectedLogErrors = unexpected(
      this.logErrors,
      allowLogErrors,
      (entry) => `${entry.text} ${entry.url ?? ''}`,
    );
    const unexpectedHttpErrors = unexpected(
      this.httpErrors,
      allowHttpErrors,
      (entry) => `${entry.status} ${entry.url}`,
    );
    // Chromium reports a renderer-canceled request (`canceled: true`) for a fetch whose
    // response was already received, notably for a DELETE answered 204 (observed on the
    // Chromium 156 headless shell, even without a follow-up request). That is DevTools
    // bookkeeping, not a network failure: the JS promise received the status and the DOM
    // assertion is authoritative. Keep every other failure.
    const isClientCanceledAfterResponse = (entry) =>
      entry.canceled === true && entry.responseStatus != null;
    const bookkeepingNetworkFailures = this.networkFailures.filter(isClientCanceledAfterResponse);
    const unexpectedNetworkFailures = this.networkFailures.filter(
      (entry) => !isClientCanceledAfterResponse(entry) && !matches(`${entry.errorText} ${entry.url}`, allowNetworkFailures),
    );

    const problems = [];
    if (unexpectedExceptions.length) {
      problems.push(
        `exceptions non capturées (${unexpectedExceptions.length}): ` +
          unexpectedExceptions.map((e) => e.description ?? e.text).join(' | '),
      );
    }
    if (unexpectedConsoleErrors.length) {
      problems.push(
        `console.error inattendues (${unexpectedConsoleErrors.length}): ` +
          unexpectedConsoleErrors.map((e) => e.text).join(' | '),
      );
    }
    if (unexpectedLogErrors.length) {
      problems.push(
        `erreurs Log.entryAdded inattendues (${unexpectedLogErrors.length}): ` +
          unexpectedLogErrors.map((e) => e.text).join(' | '),
      );
    }
    if (unexpectedHttpErrors.length) {
      problems.push(
        `réponses HTTP >= 400 inattendues (${unexpectedHttpErrors.length}): ` +
          unexpectedHttpErrors.map((e) => `HTTP ${e.status} ${e.url}`).join(' | '),
      );
    }
    if (unexpectedNetworkFailures.length) {
      problems.push(
        `requêtes en échec inattendues (${unexpectedNetworkFailures.length}): ` +
          unexpectedNetworkFailures.map((e) => `${e.errorText} ${e.url}`).join(' | '),
      );
    }
    if (problems.length) {
      throw new Error(`console/network non propre dans la page "${this.label}":\n- ${problems.join('\n- ')}`);
    }
    return {
      exceptions: this.exceptions.length,
      consoleErrors: (this.consoleErrors ?? []).length,
      logErrors: this.logErrors.length,
      httpErrors: this.httpErrors.length,
      networkFailures: this.networkFailures.length,
      clientCanceledAfterResponse: bookkeepingNetworkFailures.length,
    };
  }

  async close() {
    if (this.#closed) return;
    this.#closed = true;
    for (const unsubscribe of this.#unsubscribers.splice(0)) unsubscribe();
    try {
      await this.#connection.send('Target.closeTarget', { targetId: this.targetId }, { timeoutMs: 5000 });
    } catch {
      // The target may already be gone (crash, browser shutdown).
    }
  }
}
