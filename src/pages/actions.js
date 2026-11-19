import { boot, invoke, listen, win, $, $$, esc, h, getSettings, saveSettings, toast, diffHtml, progressLabel, hide } from './lib/core.js';
import { TRANSLATE_LANGS } from './lib/langs.js';
import { icon, actIcon } from './lib/icons.js';
import { micToggle } from './lib/mic.js';
document.querySelectorAll('[data-icon]').forEach((b) => b.insertAdjacentHTML('afterbegin', icon(b.dataset.icon, 15)));


let S = await boot({ popup: true, hideOnBlur: true });
document.addEventListener('settings', (e) => (S = e.detail));

let cap = {};             // capture from Rust: text, hwnd, app, source, auto_action, lang
let mode = 'actions';     // 'actions' | 'langs' | 'workflows'
let items = [];           // rows currently shown
let sel = 0;
let versions = [];        // result versions
let vi = 0;
let current = null;       // { action, name, lang, instruction, output }
let original = '';
let showDiff = false;
let running = false;

const src = $('#src'), q = $('#q');

listen('actions-open', open);
open();

$('#close').onclick = hide;
src.oninput = () => ($('#srcCount').textContent = src.value ? `${src.value.length} chars` : '');
q.oninput = () => { sel = 0; draw(); };
$('#send').onclick = () => enter();
$('#improve').onclick = async () => {
  if (!q.value.trim()) return toast('Type an instruction first, then ✨ improves it');
  $('#improve').disabled = true;
  try { q.value = await invoke('improve_text', { text: q.value, kind: 'instruction' }); } catch (e) { toast('Failed to improve instruction', String(e), true); }
  $('#improve').disabled = false;
};
// speak an instruction instead of typing it
$('#mic').onclick = () => micToggle($('#mic'), (t) => { q.value = t; run({ action: 'custom', instruction: t }); }, S);
$('#refineMic').onclick = () => micToggle($('#refineMic'), (t) => run(current, t), S);
$$('#outMode button').forEach((b) => (b.onclick = async () => { S = await saveSettings({ output_window: b.dataset.v === '1' }); markOut(); }));
const markOut = () => $$('#outMode button').forEach((b) => b.classList.toggle('on', (b.dataset.v === '1') === S.output_window));

async function open() {
  cap = await invoke('get_capture');
  src.value = cap.text || '';
  src.dispatchEvent(new Event('input'));
  q.value = '';
  mode = 'actions';
  sel = 0;
  markOut();
  showPick();
  if (cap.auto_action) {
    const a = cap.auto_action;
    cap.auto_action = '';
    return run({ action: a, lang: cap.lang || '' });
  }
  setTimeout(() => (src.value ? q : src).focus(), 30);
}

// ------------------------------------------------------------------ pick list
function orderedActions() {
  const order = S.action_order || [];
  return S.actions.filter((a) => !a.hidden).sort((a, b) => {
    const ia = order.indexOf(a.id), ib = order.indexOf(b.id);
    if (ia !== -1 || ib !== -1) return (ia === -1 ? 999 : ia) - (ib === -1 ? 999 : ib);
    return b.favorite - a.favorite;
  });
}

function draw() {
  const term = q.value.trim().toLowerCase();
  const list = $('#list');
  if (mode === 'langs') {
    const langs = TRANSLATE_LANGS.filter((l) => l.toLowerCase().includes(term));
    items = langs.map((l) => ({ kind: 'lang', id: l, name: l }));
    list.innerHTML = `<div class="group-label">Translate to</div>` + (items.length ? `<div class="lang-grid">${items.map((it, i) => `<div class="item ${i === sel ? 'active' : ''}" data-i="${i}">${esc(it.name)}</div>`).join('')}</div>` : '<p class="faint" style="padding:10px">No matching language.</p>');
  } else if (mode === 'workflows') {
    items = S.workflows.filter((w) => w.name.toLowerCase().includes(term)).map((w) => ({ kind: 'workflow', id: w.id, name: w.name, emo: w.icon, meta: `${w.steps.length} steps` }));
    list.innerHTML = `<div class="group-label">Choose a workflow</div>` + rows(items);
  } else {
    const acts = orderedActions().filter((a) => !term || a.name.toLowerCase().includes(term) || a.id.includes(term));
    items = acts.map((a) => ({ kind: 'action', id: a.id, name: a.name, a, meta: a.builtin ? '' : 'Custom', fav: a.favorite }));
    if (!term || 'translate to'.includes(term)) items.splice(Math.min(2, items.length), 0, { kind: 'translate', id: 'translate_to', name: 'Translate to…', ic: 'globe', meta: '›' });
    if (!term || 'run a workflow'.includes(term)) items.push({ kind: 'wf', id: 'wf', name: 'Run a workflow', ic: 'flows', meta: '›' });
    if (term) items.push({ kind: 'custom', id: 'custom', name: `Run “${q.value.trim()}”`, ic: 'sparkles', meta: '' });
    const favs = items.filter((i) => i.fav), rest = items.filter((i) => !i.fav);
    items = [...favs, ...rest];
    list.innerHTML = (favs.length && !term ? `<div class="group-label">Favorites</div>${rows(favs, 0)}<div class="group-label">Actions</div>${rows(rest, favs.length)}` : rows(items));
  }
  sel = Math.min(sel, Math.max(0, items.length - 1));
  $$('[data-i]', list).forEach((r) => {
    r.classList.toggle('active', +r.dataset.i === sel);
    r.onclick = () => { sel = +r.dataset.i; enter(); };
    r.onmousemove = () => { if (sel !== +r.dataset.i) { sel = +r.dataset.i; $$('[data-i]', list).forEach((x) => x.classList.toggle('active', +x.dataset.i === sel)); } };
  });
  $('[data-i].active', list)?.scrollIntoView({ block: 'nearest' });
}
const WF_ICON = { meeting_notes: 'fileText', email_responder: 'mail', translate_polish: 'globe' };
const rowIcon = (it) => it.a ? actIcon(it.a) : it.ic ? icon(it.ic) : it.kind === 'workflow' && WF_ICON[it.id] ? icon(WF_ICON[it.id]) : `<span class="emo">${esc(it.emo || '•')}</span>`;
const rows = (arr, offset = 0) => arr.map((it, i) => `<div class="item" data-i="${i + offset}"><span class="ico">${rowIcon(it)}</span><span class="name">${esc(it.name)}</span><span class="meta">${it.fav ? icon('star', 12) : ''}${esc(it.meta || '')}<kbd class="enter">↵</kbd></span></div>`).join('');

function enter() {
  const it = items[sel];
  const term = q.value.trim();
  if (!it) { if (term) run({ action: 'custom', instruction: term }); return; }
  if (it.kind === 'translate') { mode = 'langs'; q.value = ''; q.placeholder = 'Type to search a language…'; sel = 0; return draw(); }
  if (it.kind === 'wf') { mode = 'workflows'; q.value = ''; q.placeholder = 'Search workflows…'; sel = 0; return draw(); }
  if (it.kind === 'lang') return run({ action: 'translate', lang: it.id });
  if (it.kind === 'workflow') {
    if (!src.value.trim()) return toast('Select some text first');
    invoke('open_workflow', { id: it.id, text: src.value }).catch((e) => toast("Couldn't open the workflow", String(e), true));
    return hide();
  }
  if (it.kind === 'custom') return run({ action: 'custom', instruction: term });
  run({ action: it.id });
}

document.addEventListener('keydown', (e) => {
  if (!$('#result').classList.contains('hidden')) {
    if (e.key === 'Escape' && document.activeElement !== $('#refine')) { e.preventDefault(); return showPick(); }
    if (e.key === 'Enter' && e.ctrlKey) { e.preventDefault(); $('#insert').click(); }
    if (e.key === 'Backspace' && document.activeElement === document.body) showPick();
    return;
  }
  if (document.activeElement === src) return;
  if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(sel + (mode === 'langs' ? 3 : 1), items.length - 1); draw(); }
  else if (e.key === 'ArrowUp') { e.preventDefault(); sel = Math.max(sel - (mode === 'langs' ? 3 : 1), 0); draw(); }
  else if (mode === 'langs' && e.key === 'ArrowRight') { e.preventDefault(); sel = Math.min(sel + 1, items.length - 1); draw(); }
  else if (mode === 'langs' && e.key === 'ArrowLeft') { e.preventDefault(); sel = Math.max(sel - 1, 0); draw(); }
  else if (e.key === 'Enter') { e.preventDefault(); enter(); }
  else if (e.key === 'Escape' && mode !== 'actions') { e.preventDefault(); mode = 'actions'; q.placeholder = 'Search actions or describe a change…'; q.value = ''; draw(); }
});

function showPick() {
  // always come back to the main action list (not the language or workflow list)
  mode = 'actions';
  running = false;
  reqId++;
  q.value = '';
  q.placeholder = 'Search actions or describe a change…';
  $$('.pick-only').forEach((x) => x.classList.remove('hidden'));
  $$('.res-only').forEach((x) => x.classList.add('hidden'));
  $('#pick').classList.remove('hidden');
  $('#result').classList.add('hidden');
  $('#title').textContent = 'Quick Actions';
  draw();
}

// ------------------------------------------------------------------ run + result
let streamBuf = '';
let reqId = 0; // id of the newest request; pieces of older answers are ignored
listen('ai-delta', (e) => {
  if (!running || e.payload.req !== reqId) return;
  streamBuf += e.payload.text;
  $('#out').textContent = streamBuf;
  $('#out').scrollTop = $('#out').scrollHeight;
});

async function run(spec, refineInstr = null) {
  const text = src.value;
  const needsText = !(spec.action === 'custom');
  if (needsText && !text.trim()) return toast('Select some text first', 'Highlight text, then press the shortcut — or paste it above.');
  const action = S.actions.find((a) => a.id === spec.action);
  current = { ...spec, name: spec.action === 'translate' && spec.lang ? `Translate to ${spec.lang}` : action?.name || (spec.action === 'summarize_page' ? 'Summarize page' : spec.instruction) };
  if (!refineInstr) { versions = []; original = text; }
  $('#pick').classList.add('hidden');
  $('#result').classList.remove('hidden');
  $$('.pick-only').forEach((x) => x.classList.add('hidden'));
  $$('.res-only').forEach((x) => x.classList.remove('hidden'));
  $('#title').textContent = 'Result';
  $('#resTitle').textContent = current.name;
  $('#out').innerHTML = `<div class="loading"><div class="spinner"></div><span class="shimmer">${esc(progressLabel(spec.action))}</span><span class="faint small">usually under 2s</span></div>`;
  $('#saveAct').classList.toggle('hidden', spec.action !== 'custom' || !text.trim());
  const outKind = action?.output || (spec.action === 'custom' && !text.trim() ? 'add' : spec.action === 'custom' ? 'replace' : 'add');
  current.output = outKind;
  $('#insert').textContent = outKind === 'add' ? 'Add to my text' : 'Replace';
  $('#promise').textContent = outKind === 'add' ? 'Adds new text where your cursor is' : 'Replaces the selected text';
  running = true; streamBuf = '';
  const my = ++reqId;
  try {
    const out = await invoke('run_action', refineInstr
      ? { action: spec.action, text: original, lang: spec.lang || null, instruction: spec.instruction || null, previous: versions[vi], refine: refineInstr, req: my }
      : { action: spec.action, text, lang: spec.lang || null, instruction: spec.instruction || null, previous: null, refine: null, req: my });
    if (my !== reqId) return; // a newer request started meanwhile – drop this answer
    running = false;
    versions.push(out); vi = versions.length - 1;
    showDiff = ['fix', 'professional', 'friendly', 'shorten', 'simplify'].includes(spec.action) && !refineInstr;
    paint();
    if (!S.output_window && !refineInstr) return insert();
  } catch (e) {
    if (my !== reqId) return;
    running = false;
    $('#out').innerHTML = `<div class="err"><b>Couldn't do that</b><span class="muted">${esc(e)}</span><div style="margin-top:12px"><button class="btn" id="retry">${icon('refresh', 14)} Try again</button></div></div>`;
    $('#retry').onclick = () => run(spec, refineInstr);
  }
}

function paint() {
  const out = versions[vi] ?? '';
  if (showDiff && original) {
    const d = diffHtml(original, out);
    $('#out').innerHTML = d.html;
    $('#changes').textContent = d.changes === 0 ? 'No changes needed' : d.changes === 1 ? '1 change' : `${d.changes} changes`;
  } else {
    $('#out').textContent = out;
    $('#changes').textContent = '';
  }
  $('#vPlain').classList.toggle('on', !showDiff);
  $('#vDiff').classList.toggle('on', showDiff);
  $('#vNum').textContent = versions.length > 1 ? `${vi + 1}/${versions.length}` : '';
  $('#prevV').disabled = vi <= 0;
  $('#nextV').disabled = vi >= versions.length - 1;
}
$('#vPlain').onclick = () => { showDiff = false; paint(); };
$('#vDiff').onclick = () => { showDiff = true; paint(); };
$('#prevV').onclick = () => { if (vi > 0) { vi--; paint(); } };
$('#nextV').onclick = () => { if (vi < versions.length - 1) { vi++; paint(); } };
$('#back').onclick = showPick;
$$('#chips .chip').forEach((c) => (c.onclick = () => run(current, c.dataset.r)));
const refine = () => { const v = $('#refine').value.trim(); if (v) { $('#refine').value = ''; run(current, v); } };
$('#refineGo').onclick = refine;
$('#refine').onkeydown = (e) => e.key === 'Enter' && !e.shiftKey && (e.preventDefault(), refine());
$('#copy').onclick = async () => { await invoke('copy_text', { text: versions[vi] }); toast('On your clipboard'); };
$('#insert').onclick = insert;
$('#saveAct').onclick = async () => {
  const name = prompt('Name for this action:', current.instruction.slice(0, 40));
  if (!name) return;
  try { S = await invoke('save_action', { action: { id: '', name, prompt: current.instruction, icon: '✨', builtin: false, hidden: false, favorite: false, output: 'replace' } }); toast('Saved to your actions'); }
  catch (e) { toast('Could not save it', String(e), true); }
};

async function insert() {
  const text = versions[vi];
  if (!text) return;
  if (cap.source === 'home' || cap.source === 'manual' || !cap.hwnd) {
    await invoke('copy_text', { text });
    toast('Copied. Paste in your app');
    return;
  }
  $('#insert').textContent = 'Inserting…';
  try { await invoke('insert_result', { text, mode: current.output === 'add' ? 'add' : 'replace', original, action: current.action }); }
  catch (e) { toast('Try Insert again', String(e), true); }
}
