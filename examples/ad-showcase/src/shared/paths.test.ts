import { describe, expect, it } from 'vitest';

import { homeRelative, redactHome, shellPath } from './paths.js';

describe('homeRelative', () => {
  it('writes a path inside the home folder with ~', () => {
    expect(homeRelative('/home/u/.config/App/exports/t.json', '/home/u')).toBe(
      '~/.config/App/exports/t.json',
    );
    expect(homeRelative('/home/u/x', '/home/u/')).toBe('~/x');
    expect(homeRelative('/home/u', '/home/u')).toBe('~');
  });

  it('leaves other paths, and look-alike prefixes, unchanged', () => {
    expect(homeRelative('/home/user2/x', '/home/u')).toBe('/home/user2/x');
    expect(homeRelative('/opt/app', '/home/u')).toBe('/opt/app');
    expect(homeRelative('/opt/app', '')).toBe('/opt/app');
  });

  it('compares Windows paths without regard to case or separator', () => {
    expect(homeRelative('C:\\Users\\Me\\AppData\\Roaming\\App', 'c:\\users\\me')).toBe(
      '~\\AppData\\Roaming\\App',
    );
    expect(homeRelative('C:/Users/Me/x', 'C:\\Users\\Me')).toBe('~\\x');
    expect(homeRelative('D:\\Data\\App', 'C:\\Users\\Me')).toBe('D:\\Data\\App');
  });
});

describe('shellPath', () => {
  it('turns ~ into $HOME inside the quotes', () => {
    expect(shellPath('~/Library/Application Support/App/parity-report.json')).toBe(
      '"$HOME/Library/Application Support/App/parity-report.json"',
    );
    expect(shellPath('~')).toBe('"$HOME"');
    expect(shellPath('/opt/app/report.json')).toBe('"/opt/app/report.json"');
  });
});

describe('redactHome', () => {
  it('removes the home folder from JSON, URLs and messages', () => {
    const home = '/Users/me';
    expect(redactHome('{"url":"file:///Users/me/app/index.html"}', home)).toBe(
      '{"url":"file://~/app/index.html"}',
    );
    expect(redactHome('ENOENT: /Users/me/x.json', home)).toBe('ENOENT: ~/x.json');
    expect(redactHome('"/Users/me"', home)).toBe('"~"');
    expect(redactHome('/Users/me', home)).toBe('~');
  });

  it('leaves a longer folder name with the same start alone', () => {
    expect(redactHome('/Users/meadow/x', '/Users/me')).toBe('/Users/meadow/x');
  });

  it('handles Windows homes in plain, JSON and URL forms', () => {
    const home = 'C:\\Users\\Me Too';
    expect(redactHome('C:\\Users\\Me Too\\AppData', home)).toBe('~\\AppData');
    expect(redactHome('{"p":"c:\\\\users\\\\me too\\\\x"}', home)).toBe('{"p":"~\\\\x"}');
    expect(redactHome('file:///C:/Users/Me%20Too/app', home)).toBe('file:///~/app');
  });

  it('does nothing without a home folder', () => {
    expect(redactHome('/Users/me/x', '')).toBe('/Users/me/x');
    expect(redactHome('/x', '/')).toBe('/x');
  });
});
