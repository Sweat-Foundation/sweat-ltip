// progress-logger.ts
import { AsyncLocalStorage } from 'async_hooks';
import ora, { Ora, Options as OraOptions } from 'ora';
import logUpdate from 'log-update';
import { WriteStream } from 'node:tty';

/** Progress rendering mode */
export type ProgressMode = 'spinner' | 'line';

/** Controller returned by beginProgress/createProgress* */
export interface ProgressController {
  readonly mode: ProgressMode;
  readonly prefix: string;
  readonly active: boolean;
  /** Replace the whole line/text after the prefix. */
  set(text: string): void;
  /** Append to the existing text. */
  append(more: string): void;
  /** Finalize the progress line/spinner. Safe to call multiple times. */
  end(): void;
}

/** Options for begin/runWithProgress */
export interface ProgressOptions {
  mode?: ProgressMode;          // default: 'spinner'
  prefix?: string;              // default: ''
  spinner?: OraOptions;         // forwarded to ora
  /** Disable TTY features (force plain console). Default: autodetect (CI disables). */
  forcePlain?: boolean;
}

/* ---------------- Internals ---------------- */

type Store = { controller?: ProgressController | null };

const als = new AsyncLocalStorage<Store>();

// Simple TTY/CI detection
function isTTYEnabled(forcePlain?: boolean): boolean {
  if (forcePlain) return false;
  if (process.env.CI) return false;
  return !!process.stdout.isTTY;
}

function isInteractive(stream?: WriteStream | null, forcePlain?: boolean): boolean {
  if (forcePlain) return false;
  if (!stream) return false;
  return !!(stream as any).isTTY && !process.env.CI;
}

/* ANSI helpers (optional—useful for bold prefixes, if you want) */
export const ansi = {
  bold: (s: string) => `\x1b[1m${s}\x1b[0m`,
  gray: (s: string) => `\x1b[90m${s}\x1b[0m`,
  green: (s: string) => `\x1b[32m${s}\x1b[0m`,
  red: (s: string) => `\x1b[31m${s}\x1b[0m`,
};

/* -------------- Controllers --------------- */

function createSpinner(prefix: string, opts?: OraOptions, forcePlain?: boolean): ProgressController {
  const enabled = isTTYEnabled(forcePlain);

  if (!enabled) {
    // Plain fallback: log every update as its own line
    let current = '';
    return {
      mode: 'spinner',
      prefix,
      get active() { return true; },
      set(text: string) { current = text; console.log(prefix + current); },
      append(more: string) { current += more; console.log(prefix + current); },
      end() { /* noop: we've been logging each update already */ },
    };
  }

  // TTY path: real spinner
  const spinner: Ora = ora({ isEnabled: true, spinner: 'dots', ...opts }).start();
  let current = '';
  function render() {
    if (!spinner.isSpinning) spinner.start();
    spinner.text = prefix + current;
  }
  return {
    mode: 'spinner',
    prefix,
    get active() { return true; },
    set(text: string) { current = text; render(); },
    append(more: string) { current += more; render(); },
    end() { spinner.stop(); },
  };
}

function createLine(prefix: string, forcePlain?: boolean): ProgressController {
  const enabled = isTTYEnabled(forcePlain);
  let current = '';

  function draw() {
    if (enabled) logUpdate(prefix + current);
    else console.log(prefix + current);
  }

  return {
    mode: 'line',
    prefix,
    get active() { return true; },
    set(text: string) { current = text; draw(); },
    append(more: string) { current += more; draw(); },
    end() {
      if (enabled) logUpdate.done();
      else if (current) console.log(prefix + current);
      // idempotent
    },
  };
}

/* -------------- Public API --------------- */

/**
 * Begin a progress context bound to the current async chain.
 * Use setProgress/appendProgress/endProgress without passing a controller.
 */
export function beginProgress(opts: ProgressOptions = {}): ProgressController {
  const { mode = 'spinner', prefix = '', spinner, forcePlain } = opts;

  const controller =
    mode === 'spinner'
      ? createSpinner(prefix, spinner, forcePlain)
      : createLine(prefix, forcePlain);

  // Stash controller in ALS so helpers can find it
  const store = als.getStore() ?? {};
  store.controller = controller;
  als.enterWith(store);
  return controller;
}

/**
 * Run an async function within a progress context.
 * The progress is automatically ended in finally.
 */
export async function runWithProgress<T>(
  fn: () => Promise<T>,
  opts: ProgressOptions = {}
): Promise<T> {
  const store: Store = {};
  return await new Promise<T>((resolve, reject) => {
    als.run(store, async () => {
      const ctrl = beginProgress(opts);
      try {
        const val = await fn();
        resolve(val);
      } catch (err) {
        reject(err);
      } finally {
        ctrl.end();
      }
    });
  });
}

/** Update the current progress line/spinner in this async context. */
export function setProgress(text: string): void {
  const ctrl = als.getStore()?.controller;
  if (ctrl) ctrl.set(text);
  else console.log(text);
}

/** Append to the current progress line/spinner in this async context. */
export function appendProgress(more: string): void {
  const ctrl = als.getStore()?.controller;
  if (ctrl) ctrl.append(more);
  else console.log(more);
}

/** End the current progress line/spinner in this async context. */
export function endProgress(): void {
  const ctrl = als.getStore()?.controller;
  if (ctrl) ctrl.end();
}

/**
 * Create a controller without ALS (imperative style).
 * You manage its lifetime and call set/append/end yourself.
 */
export function createProgress(
  mode: ProgressMode = 'spinner',
  prefix = '',
  spinner?: OraOptions,
  forcePlain?: boolean
): ProgressController {
  return mode === 'spinner'
    ? createSpinner(prefix, spinner, forcePlain)
    : createLine(prefix, forcePlain);
}
