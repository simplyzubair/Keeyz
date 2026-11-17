import { boot, invoke, listen, win, $, $$, esc, toast, hide, kbd } from './lib/core.js';
import { icon, actIcon } from './lib/icons.js';
document.querySelectorAll('[data-icon]').forEach((b) => b.insertAdjacentHTML('afterbegin', icon(b.dataset.icon, 15)));


let S = await boot({ popup: true, hideOnBlur: true });
document.addEventListener('settings', (e) => (S = e.detail));
const q = $('#q');
let items = [], sel = 0, clips = [], tab = 'search', chat = [], streaming = null;

win.onFocusChanged(({ payload }) => payload && onShow());
onShow();
async function onShow() {
  clips = await invoke('clip_list').catch(() => []);
  if (tab === 'search') { q.value = ''; draw(); }
  setTimeout(() => (tab === 'chat' ? $('#chatIn') : q).focus(), 20);
}

$$('.tabs button').forEach((b) => (b.onclick = () => setTab(b.dataset.tab)));
function setTab(t) {
  tab = t;
  $$('.tabs button').forEach((b) => b.classList.toggle('on', b.dataset.tab === t));
  $('#list').classList.toggle('hidden', t !== 'search');
  $('#chat').classList.toggle('hidden', t !== 'chat');
  q.classList.toggle('hidden', t === 'chat');
  if (t === 'chat') {
    if (q.value.trim()) { $('#chatIn').value = q.value.trim(); q.value = ''; send(); }
    $('#chatIn').focus();
  } else q.focus();
}

function calc(s) {
  if (!/^[\d\s+\-*/().,%^x×÷]+$/.test(s) || !/\d/.test(s) || !/[+\-*/%^x×÷]/.test(s)) return null;
  try {
    const expr = s.replace(/×|x/g, '*').replace(/÷/g, '/').replace(/\^/g, '**').replace(/(\d+(?:\.\d+)?)%/g, '($1/100)').replace(/,/g, '');
    const v = Function(`"use strict";return (${expr})`)();
    return Number.isFinite(v) ? +v.toFixed(10) : null;
  } catch { return null; }
}

function draw() {
  const term = q.value.trim();
  const t = term.toLowerCase();
  const tools = [
    ['tool', 'actions', 'Open Quick Actions', 'bolt', S.shortcuts.actions],
    ['tool', 'voice', 'Voice input', 'mic', S.shortcuts.voice],
    ['tool', 'clipboard', 'Clipboard history', 'clipboard', S.shortcuts.clipboard],
    ['tool', 'ocr', 'Text from screen', 'scan', S.shortcuts.ocr],
    ['tool', 'summarize', 'Summarize page', 'fileText', S.shortcuts.summarize],
    ['tool', 'settings', 'Settings', 'settings', S.shortcuts.show_app],
  ].map(([kind, id, name, ic, key]) => ({ kind, id, name, ic, meta: key }));
  const acts = S.actions.filter((a) => !a.hidden).map((a) => ({ kind: 'action', id: a.id, name: a.name, a, meta: a.builtin ? '' : 'Custom' }));
  const wfs = S.workflows.map((w) => ({ kind: 'workflow', id: w.id, name: w.name, emo: w.icon, meta: `${w.steps.length} steps` }));
  const match = (i) => !t || i.name.toLowerCase().includes(t);
  const groups = [];
  const c = calc(term);
  if (c !== null) groups.push(['Calculator', [{ kind: 'calc', id: String(c), name: `= ${c.toLocaleString()}`, ic: 'calc', meta: 'Enter to copy', big: true }]]);
  groups.push(['Tools', tools.filter(match)]);
  groups.push(['Actions', acts.filter(match).slice(0, t ? 20 : 6)]);
  groups.push(['Workflows', wfs.filter(match)]);
  if (t) groups.push(['Clipboard', clips.filter((x) => x.kind === 'text' && x.text.toLowerCase().includes(t)).slice(0, 5).map((x) => ({ kind: 'clip', id: x.id, name: x.text.replace(/\s+/g, ' ').slice(0, 90), ic: 'clipboard', meta: 'Paste' }))]);
  if (t) groups.push(['Ask AI', [{ kind: 'ask', id: term, name: `Ask AI: “${term}”`, ic: 'chat', meta: 'Tab' }]]);
  items = [];
  let html = '';
  for (const [label, arr] of groups) {
    if (!arr.length) continue;
    html += `<div class="group-label">${label}</div>`;
    for (const it of arr) {
      const ico = it.a ? actIcon(it.a) : it.ic ? icon(it.ic) : `<span class="emo">${esc(it.emo || '•')}</span>`;
      html += `<div class="item ${it.big ? 'big' : ''}" data-i="${items.length}"><span class="ico">${ico}</span><span class="name ${it.big ? 'calc' : ''}">${esc(it.name)}</span><span class="meta">${it.kind === 'tool' ? (it.meta ? `<span class="kbds">${kbd(it.meta)}</span>` : '') : esc(it.meta || '')}</span></div>`;
      items.push(it);
    }
  }
  $('#list').innerHTML = html || '<p class="faint" style="padding:12px">No matching actions. Press Enter to ask AI.</p>';
  $('#count').textContent = `${items.length} results`;
  sel = Math.min(sel, Math.max(items.length - 1, 0));
  $$('[data-i]').forEach((r) => {
    r.classList.toggle('active', +r.dataset.i === sel);
    r.onclick = () => { sel = +r.dataset.i; run(); };
  });
  $('.item.active')?.scrollIntoView({ block: 'nearest' });
}

q.oninput = () => { sel = 0; draw(); };
document.addEventListener('keydown', (e) => {
  if (e.key === 'Tab') { e.preventDefault(); setTab(tab === 'search' ? 'chat' : 'search'); return; }
  if (tab !== 'search') return;
  if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(sel + 1, items.length - 1); draw(); }
  if (e.key === 'ArrowUp') { e.preventDefault(); sel = Math.max(sel - 1, 0); draw(); }
  if (e.key === 'Enter') { e.preventDefault(); run(); }
});

async function run() {
  const it = items[sel];
  if (!it) { if (q.value.trim()) setTab('chat'); return; }
  switch (it.kind) {
    case 'calc': await invoke('copy_text', { text: it.id }); toast('Copied', it.id); return hide();
    case 'tool':
      if (it.id === 'summarize') { await hide(); return invoke('launcher_action', { kind: 'action', id: 'summarize_page' }); }
      await hide(); return invoke('open_tool_cmd', { tool: it.id });
    case 'action': await hide(); return invoke('launcher_action', { kind: 'action', id: it.id });
    case 'workflow': await hide(); return invoke('launcher_action', { kind: 'workflow', id: it.id });
    case 'clip': return invoke('clip_paste', { id: it.id }).then(hide);
    case 'ask': return setTab('chat');
  }
}

// ---------------------------------------------------------------- chat
listen('ai-delta', (e) => {
  if (!streaming) return;
  streaming.content += e.payload;
  streaming.el.querySelector('.txt').textContent = streaming.content;
  $('#msgs').scrollTop = $('#msgs').scrollHeight;
});
$('#chatSend').onclick = send;
$('#chatIn').onkeydown = (e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } };

function bubble(role, text) {
  $('#chatHero')?.remove();
  const el = document.createElement('div');
  el.className = `msg ${role}`;
  el.innerHTML = `<div class="txt">${esc(text)}</div>`;
  $('#msgs').appendChild(el);
  $('#msgs').scrollTop = $('#msgs').scrollHeight;
  return el;
}

async function send() {
  const text = $('#chatIn').value.trim();
  if (!text || streaming) return;
  $('#chatIn').value = '';
  chat.push({ role: 'user', content: text });
  bubble('user', text);
  const el = bubble('assistant', '…');
  streaming = { el, content: '' };
  try {
    const out = await invoke('chat', { messages: chat });
    chat.push({ role: 'assistant', content: out });
    el.querySelector('.txt').textContent = out;
    const acts = document.createElement('div');
    acts.className = 'acts';
    acts.innerHTML = '<button class="btn sm">Copy</button><button class="btn sm">Insert</button>';
    acts.children[0].onclick = async () => { await invoke('copy_text', { text: out }); toast('Reply copied to clipboard'); };
    acts.children[1].onclick = () => invoke('paste_into_last', { text: out });
    el.appendChild(acts);
  } catch (e) {
    el.querySelector('.txt').textContent = String(e);
    el.style.color = 'var(--danger)';
    chat.pop();
  }
  streaming = null;
}
