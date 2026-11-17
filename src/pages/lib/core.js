// Shared helpers for every Keeyz window.
import { invoke } from '@tauri-apps/api/core';
import { listen, emit } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

export { invoke, listen, emit };
export const win = getCurrentWindow();
export const $ = (s, el = document) => el.querySelector(s);
export const $$ = (s, el = document) => [...el.querySelectorAll(s)];

export const esc = (s) =>
  String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

export function h(html) {
  const t = document.createElement('template');
  t.innerHTML = html.trim();
  return t.content.firstElementChild;
}

let settingsCache = null;
export async function getSettings(force = false) {
  if (!settingsCache || force) settingsCache = await invoke('get_settings');
  return settingsCache;
}
export async function saveSettings(patch) {
  settingsCache = await invoke('save_settings', { patch });
  return settingsCache;
}

export function applyTheme(s) {
  const root = document.documentElement;
  let theme = s?.theme || 'system';
  if (theme === 'system') theme = matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  root.dataset.theme = theme;
  root.dataset.size = s?.text_size || 'default';
}

/** Call once per window: theme, live settings updates, sounds, toasts, no context menu. */
export async function boot({ popup = false, hideOnBlur = false } = {}) {
  const s = await getSettings();
  applyTheme(s);
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => applyTheme(settingsCache));
  listen('settings-changed', (e) => {
    settingsCache = e.payload;
    applyTheme(e.payload);
    document.dispatchEvent(new CustomEvent('settings', { detail: e.payload }));
  });
  listen('play-sound', (e) => sound(e.payload));
  document.addEventListener('contextmenu', (e) => {
    if (!e.target.closest('textarea, input')) e.preventDefault();
  });
  if (popup) {
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && !e.defaultPrevented) hide();
    });
  }
  if (hideOnBlur) {
    win.onFocusChanged(({ payload: focused }) => {
      if (!focused && !document.body.dataset.pinned) hide();
    });
  }
  return s;
}

export const hide = () => invoke('hide_me');

export function toast(title, body = '', err = false) {
  let box = document.getElementById('toasts');
  if (!box) {
    box = document.createElement('div');
    box.id = 'toasts';
    document.body.appendChild(box);
  }
  const t = h(`<div class="toast ${err ? 'err' : ''}"><b>${esc(title)}</b>${esc(body)}</div>`);
  box.appendChild(t);
  setTimeout(() => t.remove(), err ? 6000 : 3000);
}

let actx;
export function sound(kind = 'done') {
  try {
    actx ||= new AudioContext();
    const o = actx.createOscillator();
    const g = actx.createGain();
    o.connect(g).connect(actx.destination);
    const f = { done: [660, 990], start: [520, 780], error: [300, 200] }[kind] || [600, 800];
    o.frequency.setValueAtTime(f[0], actx.currentTime);
    o.frequency.linearRampToValueAtTime(f[1], actx.currentTime + 0.12);
    g.gain.setValueAtTime(0.08, actx.currentTime);
    g.gain.exponentialRampToValueAtTime(0.0001, actx.currentTime + 0.25);
    o.start();
    o.stop(actx.currentTime + 0.26);
  } catch {}
}

/** "Ctrl+Shift+A" -> <kbd> chips */
export function kbd(accel) {
  if (!accel) return '<span class="faint">Off</span>';
  return accel.split('+').map((k) => `<kbd>${esc(k)}</kbd>`).join(' ');
}

/** Turn a keydown event into an accelerator string, or null while only modifiers are held. */
export function accelFromEvent(e) {
  const mods = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey) mods.push('Super');
  const code = e.code;
  if (/^(Control|Shift|Alt|Meta|OS)/.test(code)) return null;
  let key;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit\d$/.test(code)) key = code.slice(5);
  else if (/^F\d{1,2}$/.test(code)) key = code;
  else key = { Space: 'Space', Enter: 'Enter', Backquote: '`', Minus: '-', Equal: '=', Comma: ',', Period: '.', Slash: '/', Semicolon: ';', Quote: "'", BracketLeft: '[', BracketRight: ']', Backslash: '\\' }[code] || code;
  if (!mods.length && !/^F\d/.test(key)) return null;
  return [...mods, key].join('+');
}

export function timeAgo(ms) {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** Word-level diff: returns HTML with <ins>/<del>. */
export function diffHtml(a, b) {
  const A = a.split(/(\s+)/), B = b.split(/(\s+)/);
  const n = A.length, m = B.length;
  if (n * m > 400000) return esc(b);
  const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--)
    dp[i][j] = A[i] === B[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  let i = 0, j = 0, out = '', changes = 0;
  while (i < n || j < m) {
    if (i < n && j < m && A[i] === B[j]) { out += esc(A[i]); i++; j++; }
    else if (j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j])) { if (B[j].trim()) changes++; out += `<ins class="d">${esc(B[j])}</ins>`; j++; }
    else { if (A[i].trim()) changes++; out += `<del class="d">${esc(A[i])}</del>`; i++; }
  }
  return { html: out, changes };
}

export const ACTION_LABELS = {
  fix: ['Fixing text…', 'Text fixed'], translate: ['Translating…', 'Translated'], professional: ['Making professional…', 'Made professional'],
  friendly: ['Making friendly…', 'Made friendly'], casual: ['Writing message…', 'Message written'], shorten: ['Shortening…', 'Shortened'],
  elaborate: ['Elaborating…', 'Elaborated'], summarize: ['Summarizing…', 'Summarized'], explain: ['Explaining…', 'Explained'],
  bullet_points: ['Converting to bullets…', 'Converted to bullets'], reply: ['Drafting reply…', 'Reply drafted'],
  email_reply: ['Drafting email reply…', 'Email reply drafted'], quick_reply: ['Drafting quick reply…', 'Quick reply drafted'],
  decline_reply: ['Drafting decline…', 'Decline drafted'], email: ['Formatting email…', 'Email formatted'],
  simplify: ['Simplifying…', 'Simplified'], explain_code: ['Explaining code…', 'Code explained'],
  refactor_code: ['Refactoring code…', 'Code refactored'], generate_tests: ['Generating tests…', 'Tests generated'],
  summarize_page: ['Summarizing page…', 'Page summarized'],
};
export const progressLabel = (id) => ACTION_LABELS[id]?.[0] || 'Working…';
