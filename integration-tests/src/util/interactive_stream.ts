import fs from 'node:fs';

/**
 * Return a real interactive TTY stream if possible.
 * Prefer stderr; fall back to /dev/tty (POSIX) or \\.\CON (Windows).
 */
export function getInteractiveStream(): NodeJS.WriteStream | undefined {
  if ((process.stderr as NodeJS.WriteStream).isTTY) return process.stderr as NodeJS.WriteStream;
  if ((process.stdout as NodeJS.WriteStream).isTTY) return process.stdout as NodeJS.WriteStream;

  // POSIX
  try {
    const s = fs.createWriteStream('/dev/tty') as unknown as NodeJS.WriteStream;
    (s as any).isTTY = true;
    return s;
  } catch { }

  // Windows
  try {
    const s = fs.createWriteStream('\\\\.\\CON') as unknown as NodeJS.WriteStream;
    (s as any).isTTY = true;
    return s;
  } catch { }

  return undefined;
}
