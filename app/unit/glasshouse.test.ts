import { describe, expect, it } from 'vitest';
import {
  builtInStartAddress,
  glasshouseUserAgent,
  problemOf,
  START_ADDRESS_VARIABLE,
  startAddress,
  typedAddress,
} from '../src/glasshouse';

describe('a typed address', () => {
  it('gets https when it has no scheme', () => {
    expect(typedAddress('constructor.app')).toBe('https://constructor.app/');
    expect(typedAddress('  example.com/a?b=1#c ')).toBe('https://example.com/a?b=1#c');
  });

  it('keeps http and https as typed', () => {
    expect(typedAddress('http://127.0.0.1:8080/x')).toBe('http://127.0.0.1:8080/x');
    expect(typedAddress('HTTPS://Example.com')).toBe('https://example.com/');
  });

  it('reads a port after a host as a port, not as a scheme', () => {
    expect(typedAddress('localhost:8080')).toBe('https://localhost:8080/');
    expect(typedAddress('example.com:443/path')).toBe('https://example.com/path');
  });

  it('refuses every other scheme', () => {
    for (const raw of [
      'file:///etc/hosts',
      'javascript:alert(1)',
      'data:text/html,hi',
      'ftp://example.com',
      'chrome://gpu',
      'blob:https://example.com/x',
    ]) {
      expect(typedAddress(raw), raw).toBeNull();
    }
  });

  it('refuses what is not an address', () => {
    for (const raw of ['', '   ', 'two words', 'https://', undefined, null, 5]) {
      expect(typedAddress(raw), String(raw)).toBeNull();
    }
  });
});

describe('the start address', () => {
  it('comes from the environment first', () => {
    const env = { [START_ADDRESS_VARIABLE]: 'localhost:3000' };
    expect(startAddress(env, 'https://built-in.example')).toBe('https://localhost:3000/');
  });

  it('falls back to the built-in one when the environment has none or a bad one', () => {
    expect(startAddress({}, 'https://built-in.example')).toBe('https://built-in.example/');
    expect(startAddress({ [START_ADDRESS_VARIABLE]: 'file:///x' }, 'built-in.example')).toBe(
      'https://built-in.example/',
    );
  });

  it('is none when neither is set', () => {
    expect(startAddress({}, '')).toBeNull();
  });

  it('is none when the environment sets the variable to an empty text', () => {
    expect(startAddress({ [START_ADDRESS_VARIABLE]: '' }, 'https://built-in.example')).toBeNull();
  });

  it('is the built-in address, a web address, when the environment says nothing', () => {
    expect(builtInStartAddress).toBe('http://constructorfabric.com/');
    expect(startAddress({})).toBe('http://constructorfabric.com/');
  });
});

describe('the User-Agent of the page', () => {
  const base =
    'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) ConstructBrowser/1.0.0 Chrome/150.0.1.2 Electron/45.2.3 Safari/537.36';

  it('names Construct, keeps Chrome and drops Electron and the app’s own token', () => {
    expect(glasshouseUserAgent(base, '1.0.0')).toBe(
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.1.2 Safari/537.36 Construct/1.0.0',
    );
  });
});

describe('what a failed load means', () => {
  it('tells a certificate from an address that is not found', () => {
    expect(problemOf(-202).kind).toBe('certificate');
    expect(problemOf(-200).kind).toBe('certificate');
    expect(problemOf(-105).kind).toBe('not-found');
    expect(problemOf(-102).kind).toBe('refused');
    expect(problemOf(-106).kind).toBe('offline');
    expect(problemOf(-7).kind).toBe('other');
  });
});
