import { boot, invoke, listen, win, $, $$, esc, h, getSettings, saveSettings, toast, kbd, accelFromEvent, timeAgo, diffHtml, progressLabel } from './lib/core.js';
import { TRANSLATE_LANGS, VOICE_LANGS } from './lib/langs.js';
import { icon, actIcon } from './lib/icons.js';
import logoUrl from './lib/logo.svg';

$$('[data-icon]').forEach((b) => b.insertAdjacentHTML('afterbegin', icon(b.dataset.icon, b.classList.contains('tb-btn') ? 15 : 19)));

let S = await boot();
const TITLES = { home: 'Home', flows: 'Workflows', ai: 'AI · Actions & style', snap: 'Snap · windows', settings: 'Settings', provider: 'AI Provider', help: 'Help & setup guide', feedback: 'Send feedback' };
let current = 'home';

document.addEventListener('settings', (e) => { S = e.detail; render(current, true); });
$('#tbMin').onclick = () => win.minimize();
$('#tbClose').onclick = () => win.close();
$$('.rail-btn[data-view]').forEach((b) => (b.onclick = () => show(b.dataset.view)));
$$('.rail-btn[data-tool]').forEach((b) => (b.onclick = () => invoke('open_tool_cmd', { tool: b.dataset.tool })));
listen('navigate', (e) => show(e.payload));
listen('history-changed', () => current === 'home' && renderHomeLive());
listen('toast', (e) => toast(e.payload.title, e.payload.body));
listen('shortcut-errors', (e) => e.payload.forEach((m) => toast('Shortcut problem', m, true)));
listen('trigger-paused', (e) => setStatus(e.payload));
setStatus(await invoke('trigger_paused'));

function setStatus(paused) {
  const st = $('#status');
  st.classList.toggle('paused', !!paused);
  st.querySelector('span').textContent = paused ? 'Typed triggers paused' : 'Ready';
}

function show(view) {
  current = view;
  $$('.rail-btn[data-view]').forEach((b) => b.classList.toggle('active', b.dataset.view === view));
  $$('.view').forEach((v) => v.classList.toggle('hidden', v.id !== `view-${view}`));
  $('#viewTitle').textContent = TITLES[view] || '';
  Promise.resolve(render(view)).then(() => ($('#views').scrollTop = 0));
  $('#views').scrollTop = 0;
}

function render(view, soft = false) {
  if (soft && document.activeElement?.closest('.view') && view !== 'home') return; // don't wipe what the user is typing
  ({ home: renderHome, flows: renderFlows, ai: renderAI, snap: renderSnap, settings: renderSettings, provider: renderProvider, help: renderHelp, feedback: renderFeedback })[view]?.();
}

// ============================================================ UPDATES
let pendingUpdate = null;
function showUpdate(u) {
  pendingUpdate = u;
  $('#updPill').classList.remove('hidden');
  $('#updPill').textContent = `Update to ${u.version}`;
  if ($('#updText')) $('#updText').innerHTML = `Version <b>${esc(u.version)}</b> is ready.`;
  $('#updNow')?.classList.remove('hidden');
}
async function checkUpdate(manual = false) {
  const b = $('#updCheck');
  if (b) { b.disabled = true; b.innerHTML = '<div class="spinner"></div> Checking…'; }
  try {
    const u = await invoke('check_update');
    if (u.available) showUpdate(u);
    else if (manual) { toast("You're up to date", `Version ${u.current} is the newest.`); if ($('#updText')) $('#updText').textContent = 'This is the newest version.'; }
  } catch (e) { if (manual) toast("Couldn't check for updates", String(e), true); }
  if (b) { b.disabled = false; b.innerHTML = `${icon('refresh', 13)} Check for updates`; }
}
async function installUpdate() {
  if (!pendingUpdate) return checkUpdate(true);
  for (const el of [$('#updPill'), $('#updNow')]) if (el) { el.disabled = true; el.textContent = 'Updating…'; }
  toast('Updating Keeyz', 'It restarts by itself in a moment. Your settings stay.');
  try { await invoke('install_update'); } catch (e) {
    toast('Update failed', String(e), true);
    for (const el of [$('#updPill'), $('#updNow')]) if (el) { el.disabled = false; el.textContent = 'Update now'; }
  }
}
$('#updPill').onclick = installUpdate;
listen('update-available', (e) => showUpdate(e.payload));
listen('check-update', () => checkUpdate(true));
listen('install-update', () => installUpdate());
listen('run-tour', () => welcome(true));
listen('update-progress', (e) => { for (const el of [$('#updPill'), $('#updNow')]) if (el) el.textContent = `Updating… ${e.payload}%`; });

// ============================================================ HOME
async function renderHome() {
  const el = $('#view-home');
  const [stats, history, keys] = await Promise.all([invoke('get_stats'), invoke('get_history'), invoke('key_status')]);
  // Auto uses any saved key; a local / custom model needs none
  const hasKey = S.provider === 'custom' || (S.provider === 'auto' ? Object.values(keys).some((k) => k?.set) : keys[S.provider]?.set);
  const sc = S.shortcuts;
  el.innerHTML = `
    ${hasKey ? '<div style="height:10px"></div>' : `<div class="notice"><span class="n-ico">${icon('key', 17)}</span><div class="grow"><b>Add your AI key to start</b><div class="muted small">Keeyz runs on your own OpenAI, Anthropic, Gemini, OpenRouter, DeepSeek or local model key.</div></div>
      <button class="btn primary" id="goKey">Add key ${icon('arrowRight', 14)}</button></div>`}
    <div class="hero">
      <div class="eyebrow">${icon('sparkles', 13)} Try it here</div>
      <h1>Select text anywhere, press ${kbd(sc.actions)}</h1>
      <div class="sample" id="sample" contenteditable="true" spellcheck="false">hey can u send me the report when you get a chance</div>
      <div class="row"><button class="btn light" id="tryFix">${icon('check', 14)} Fix it here</button><button class="btn" id="tryQA">${icon('bolt', 14)} Try it in Quick Actions</button><span class="spacer"></span><span class="under">This sentence is yours to try on</span><button class="btn ghost icon" id="tryReset" title="Reset">${icon('refresh', 14)}</button></div>
    </div>
    <div class="command">${icon('sparkles', 17)}
      <input id="cmd" placeholder="What do you want to write? e.g. a thank-you note to my team" />
      <button class="btn primary" id="cmdRun">Run ${icon('arrowRight', 14)}</button>
    </div>
    <div class="hint">Or, in any app: type your text, then <b>${esc(S.trigger_word)}</b> + your instruction, then press space twice.
      <div class="ex"><span>Try</span><i>${esc(S.trigger_word)} fix my grammar</i><i>${esc(S.trigger_word)} translate to Spanish</i><i>${esc(S.trigger_word)} make it shorter</i></div></div>
    <div class="result-box hidden" id="cmdResult"></div>
    <h2>Tools</h2>
    <div class="tools">
      ${tool('bolt', 'Quick Actions', 'actions', sc.actions)}
      ${tool('mic', 'Voice', 'voice', sc.voice)}
      ${tool('scan', 'From screen', 'ocr', sc.ocr)}
      ${tool('clipboard', 'Clipboard', 'clipboard', sc.clipboard)}
      <div class="tool" data-view="snap"><span class="t-ico">${icon('layout', 18)}</span><b>Snap windows</b><span class="t-key"><kbd>Ctrl</kbd> <kbd>Ctrl</kbd></span></div>
    </div>
    <div class="hint">From any app, ${kbd(sc.launcher)} opens the Launcher: search every tool, action and workflow in one box.</div>
    <div id="homeLive"></div>`;
  $('#goKey')?.addEventListener('click', () => show('provider'));
  $$('.tool', el).forEach((t) => (t.onclick = () => (t.dataset.view ? show(t.dataset.view) : invoke('open_tool_cmd', { tool: t.dataset.tool }))));
  renderHomeLive();
  const sample = $('#sample');
  $('#tryReset').onclick = () => (sample.textContent = 'hey can u send me the report when you get a chance');
  $('#tryFix').onclick = async () => {
    const b = $('#tryFix'); b.disabled = true; b.textContent = 'Fixing…';
    try {
      const before = sample.textContent;
      const out = await invoke('run_action', { action: 'fix', text: before });
      sample.innerHTML = diffHtml(before, out).html;
      b.innerHTML = `${icon('check', 14)} Fixed`;
    } catch (e) { toast("Couldn't fix it", String(e), true); b.innerHTML = `${icon('check', 14)} Fix it here`; }
    b.disabled = false;
  };
  $('#tryQA').onclick = () => invoke('open_actions_with', { text: sample.textContent, source: 'home', action: null });
  const run = async () => {
    const q = $('#cmd').value.trim();
    if (!q) return;
    const box = $('#cmdResult');
    box.classList.remove('hidden');
    box.innerHTML = `<div class="card"><div class="row"><div class="spinner"></div><span class="shimmer">Writing…</span></div></div>`;
    try {
      const out = await invoke('run_action', { action: 'custom', text: '', instruction: q });
      box.innerHTML = `<div class="card"><div class="row"><span class="badge">Result</span><span class="spacer"></span><button class="btn sm" id="cpy">${icon('copy', 13)} Copy</button></div><div class="result-text" style="margin-top:8px">${esc(out)}</div></div>`;
      $('#cpy').onclick = async () => { await invoke('copy_text', { text: out }); toast('Copied'); };
    } catch (e) { box.innerHTML = `<div class="card" style="color:var(--danger)">${esc(e)}</div>`; }
  };
  $('#cmdRun').onclick = run;
  $('#cmd').onkeydown = (e) => e.key === 'Enter' && run();
}
// Activity + Recent: refreshed on its own after each AI job, so the rest of Home (your test text,
// the command box answer) is never wiped.
async function renderHomeLive() {
  const box = $('#homeLive');
  if (!box) return;
  const [stats, history] = await Promise.all([invoke('get_stats'), invoke('get_history')]);
  box.innerHTML = `
    <h2>Activity</h2>
    <div class="stats">
      <div class="stat"><span>Today</span><b>${stats.today}</b></div>
      <div class="stat"><span>This week</span><b>${stats.week}</b></div>
      <div class="stat"><span>All time</span><b>${stats.total}</b></div>
      <div class="stat"><span>Words written</span><b>${Number(stats.words).toLocaleString()}</b></div>
    </div>
    <div class="section-head"><h2>Recent</h2><span class="spacer"></span>
      ${history.length ? `<button class="btn ghost sm" id="clearRecent">Clear</button><button class="btn sm" id="undoBtn" title="Undo last change in your app">${icon('undo', 13)} Undo last change</button>` : ''}</div>
    <div class="${history.length ? 'recent' : ''}">${history.length ? history.map((i) => `
      <div class="recent-item"><span class="badge">${esc(i.action)}</span>
        <span class="r-text" title="${esc(i.result)}">${esc(i.result)}</span>
        <span class="faint small">${timeAgo(i.time)}</span>
        <button class="btn ghost icon sm" data-copy="${esc(i.result)}" title="Copy">${icon('copy', 14)}</button></div>`).join('') : '<div class="empty">Nothing yet. Your last 20 AI changes appear here.</div>'}</div>`;
  $$('[data-copy]', box).forEach((b) => (b.onclick = async () => { await invoke('copy_text', { text: b.dataset.copy }); toast('Copied'); }));
  $('#clearRecent')?.addEventListener('click', async () => { await invoke('clear_history'); renderHomeLive(); });
  $('#undoBtn')?.addEventListener('click', () => invoke('undo'));
}
const tool = (ico, name, id, key) => `<div class="tool" data-tool="${id}"><span class="t-ico">${icon(ico, 18)}</span><b>${name}</b><span class="t-key">${kbd(key)}</span></div>`;

// ============================================================ FLOWS (workflows)
async function renderFlows(editing = null) {
  const el = $('#view-flows');
  if (editing) return renderWorkflowEditor(el, editing);
  const yours = S.workflows.filter((w) => !w.builtin), builtIn = S.workflows.filter((w) => w.builtin);
  const card = (w) => `<div class="wf-card"><div class="w-ico">${WF_ICON[w.id] ? icon(WF_ICON[w.id], 19) : !w.icon || w.icon === '⛓' ? icon('flows', 19) : esc(w.icon)}</div>
    <div class="grow"><b>${esc(w.name)}</b><div class="muted small">${esc(w.description || '')}</div>
      <div class="wf-steps">${w.steps.map((s) => `<span class="st">${esc(s.output_label || s.mode)}</span>`).join(icon('chevronRight', 12))}</div></div>
    <div class="row"><button class="btn ghost icon sm" data-edit="${w.id}" title="${w.builtin ? 'Duplicate and edit' : 'Edit'}">${icon('edit', 14)}</button>${w.builtin ? '' : `<button class="btn ghost icon sm danger" data-del="${w.id}" title="Delete">${icon('trash', 14)}</button>`}
      <button class="btn primary sm" data-run="${w.id}">${icon('play', 12)} Run</button></div></div>`;
  el.innerHTML = `<h1>Workflows</h1>
    <p class="lead">A workflow runs several AI steps on your text in a row: summarize, then extract action items, then draft an email. Select text in any app, then run it from Quick Actions (“Run a workflow”), the Launcher, a shortcut, or paste text here.</p>
    <div class="row"><input class="input" id="wfSearch" placeholder="Search workflows…" style="max-width:320px" /><span class="spacer"></span><button class="btn primary" id="wfNew">${icon('plus', 14)} New workflow</button></div>
    <h2>Yours</h2>${yours.length ? yours.map(card).join('') : '<div class="empty">No workflows yet. Create your first one, or duplicate a built-in workflow.</div>'}
    <h2>Built in</h2>${builtIn.map(card).join('')}`;
  $('#wfNew').onclick = () => renderFlows({ id: '', name: '', description: '', icon: '⛓', builtin: false, steps: [{ mode: 'summarize', input_source: 'original_text', output_label: 'Summary', custom_prompt: '' }] });
  $('#wfSearch').oninput = (e) => {
    const q = e.target.value.toLowerCase();
    $$('.wf-card', el).forEach((c) => c.classList.toggle('hidden', !c.textContent.toLowerCase().includes(q)));
  };
  $$('[data-edit]', el).forEach((b) => (b.onclick = () => {
    const w = structuredClone(S.workflows.find((x) => x.id === b.dataset.edit));
    if (w.builtin) { w.id = ''; w.builtin = false; w.name += ' (copy)'; }
    renderFlows(w);
  }));
  $$('[data-del]', el).forEach((b) => (b.onclick = async () => { if (confirm('Delete this workflow?')) { S = await invoke('delete_workflow', { id: b.dataset.del }); renderFlows(); } }));
  $$('[data-run]', el).forEach((b) => (b.onclick = () => {
    const text = prompt('Paste the text to run this workflow on:');
    if (text?.trim()) invoke('open_workflow', { id: b.dataset.run, text });
  }));
}

function renderWorkflowEditor(el, w) {
  const modes = S.actions.map((a) => `<option value="${a.id}">${esc(a.name)}</option>`).join('') + '<option value="custom">Custom instruction</option>';
  const draw = () => {
    el.innerHTML = `<button class="btn ghost sm" id="back">${icon('chevronLeft', 14)} Back</button>
      <h1 style="margin-top:10px">${w.id ? 'Edit workflow' : 'New workflow'}</h1>
      <div class="row" style="margin:14px 0"><input class="input" id="wIcon" value="${esc(w.icon)}" style="width:64px;text-align:center" title="Icon (emoji)" />
        <input class="input grow" id="wName" placeholder="Name, e.g. Blog post pipeline" value="${esc(w.name)}" /></div>
      <input class="input" id="wDesc" placeholder="Short description" value="${esc(w.description)}" />
      <h2>Steps</h2>
      <div id="steps">${w.steps.map((s, i) => `<div class="step-edit" data-i="${i}"><span class="sn">${i + 1}</span>
        <select class="input" data-k="mode">${modes}</select>
        <select class="input" data-k="input_source"><option value="original_text">Input: your text</option><option value="previous_step">Input: previous step</option><option value="combined">Input: text + all results</option></select>
        <input class="input" data-k="output_label" placeholder="Result label" value="${esc(s.output_label)}" />
        <button class="btn ghost icon sm danger" data-rm="${i}" title="Remove step">${icon('trash', 14)}</button>
        <textarea class="input" data-k="custom_prompt" placeholder="${s.mode === 'custom' ? 'Instruction for this step' : 'Optional extra instruction'}">${esc(s.custom_prompt)}</textarea></div>`).join('')}</div>
      <button class="btn" id="addStep">${icon('plus', 14)} Add step</button>
      <div class="row" style="margin-top:20px"><span class="spacer"></span><button class="btn" id="cancel">Cancel</button><button class="btn primary" id="save">Save workflow</button></div>`;
    $$('.step-edit', el).forEach((row) => {
      const s = w.steps[row.dataset.i];
      row.querySelector('[data-k=mode]').value = s.mode;
      row.querySelector('[data-k=input_source]').value = s.input_source || 'original_text';
      $$('[data-k]', row).forEach((inp) => (inp.oninput = () => { s[inp.dataset.k] = inp.value; if (inp.dataset.k === 'mode') sync(); }));
    });
    $$('[data-rm]', el).forEach((b) => (b.onclick = () => { sync(); w.steps.splice(+b.dataset.rm, 1); draw(); }));
    $('#addStep').onclick = () => { sync(); w.steps.push({ mode: 'custom', input_source: 'previous_step', output_label: `Step ${w.steps.length + 1}`, custom_prompt: '' }); draw(); };
    $('#back').onclick = $('#cancel').onclick = () => renderFlows();
    $('#save').onclick = async () => {
      sync();
      try { S = await invoke('save_workflow', { workflow: w }); toast('Workflow saved'); renderFlows(); } catch (e) { toast("Couldn't save", String(e), true); }
    };
  };
  const sync = () => { w.name = $('#wName').value; w.icon = $('#wIcon').value; w.description = $('#wDesc').value; };
  draw();
}

// ============================================================ AI (actions + style + prompt context)
async function renderAI() {
  const el = $('#view-ai');
  const acts = orderedActions();
  const row = (a) => `<div class="act-row ${a.hidden ? 'hidden-act' : ''}" draggable="true" data-id="${a.id}">
      <span class="grip">${icon('grip', 15)}</span><span class="a-ico">${actIcon(a)}</span>
      <div class="grow"><div class="row" style="gap:6px"><b>${esc(a.name)}</b>${a.builtin ? '' : '<span class="badge">Custom</span>'}${a.hidden ? '<span class="badge neutral">Hidden</span>' : ''}</div><div class="faint small trunc">${esc(a.prompt)}</div></div>
      <div class="a-btns"><button class="btn ghost icon sm star ${a.favorite ? 'on' : ''}" data-star="${a.id}" title="Favorite">${icon('star', 15)}</button>
      <button class="btn ghost icon sm" data-hide="${a.id}" title="${a.hidden ? 'Show in popup' : 'Hide from popup'}">${icon(a.hidden ? 'eye' : 'eyeOff', 15)}</button>
      <button class="btn ghost icon sm" data-edit="${a.id}" title="Edit">${icon('edit', 15)}</button>
      ${a.builtin ? '' : `<button class="btn ghost icon sm danger" data-del="${a.id}" title="Delete">${icon('trash', 15)}</button>`}</div></div>`;
  el.innerHTML = `<h1>Quick Actions</h1>
    <p class="lead">Starred actions appear first in the popup. Hover a row to star, hide, edit, or drag it. Custom actions also show in the Launcher and can get their own shortcut.</p>
    <div class="row"><input class="input" id="actSearch" placeholder="Search actions…" style="max-width:320px" /><span class="spacer"></span><button class="btn primary" id="newAct">${icon('plus', 14)} New action</button></div>
    <div class="set-group" style="margin-top:14px" id="actList">${acts.map(row).join('')}</div>
    <h2>Your style</h2>
    <div class="set-group"><div class="set-row"><div class="s-text"><b>Use your style</b><span>Tell Keeyz how you write. Used for Reply, Email, Professional, Shorten, answers, typed trigger, voice and workflows. Fix and Translate stay faithful to your text.</span></div>${sw('style_enabled')}</div>
      <div class="set-row" style="display:block"><textarea class="input" id="styleText" placeholder="e.g. Short sentences. Friendly but professional. British English. Never use em dashes.">${esc(S.style_text)}</textarea>
      <div class="row" style="margin-top:8px;flex-wrap:wrap">${['Keep sentences short', 'Never use em dashes', 'Use British English', 'Friendly, warm tone', 'Formal, professional tone', 'Explain things in my language'].map((x) => `<span class="chip" data-ex="${x}">${icon('plus', 12)} ${x}</span>`).join('')}</div></div></div>
    <h2>Prompt context</h2>
    <div class="set-group"><div class="set-row"><div class="s-text"><b>Background for the typed trigger</b><span>Facts the AI keeps in mind for every typed trigger (your job, product names, how you sign emails). Load a .txt or .md file to fill it.</span></div>${sw('prompt_context_enabled')}</div>
      <div class="set-row" style="display:block"><textarea class="input" id="ctxText" style="min-height:110px" placeholder="e.g. I'm Zubair, I run Skillsgeek. Sign emails as 'Zubair'.">${esc(S.prompt_context)}</textarea>
      <div class="row" style="margin-top:8px"><label class="btn sm">Load file<input type="file" accept=".txt,.md" id="ctxFile" hidden /></label><button class="btn sm" id="ctxImprove">${icon('wand', 13)} Improve</button><span class="faint small" id="ctxCount">${S.prompt_context.length} chars</span><span class="spacer"></span><span class="faint small">Saved automatically</span></div></div></div>`;
  bindSwitches(el);
  $('#newAct').onclick = () => actionEditor();
  $('#actSearch').oninput = (e) => { const q = e.target.value.toLowerCase(); $$('.act-row', el).forEach((r) => r.classList.toggle('hidden', !r.textContent.toLowerCase().includes(q))); };
  const upd = async (id, patch) => { const a = { ...S.actions.find((x) => x.id === id), ...patch }; S = await invoke('save_action', { action: a }); renderAI(); };
  $$('[data-star]', el).forEach((b) => (b.onclick = () => upd(b.dataset.star, { favorite: !S.actions.find((x) => x.id === b.dataset.star).favorite })));
  $$('[data-hide]', el).forEach((b) => (b.onclick = () => upd(b.dataset.hide, { hidden: !S.actions.find((x) => x.id === b.dataset.hide).hidden })));
  $$('[data-edit]', el).forEach((b) => (b.onclick = () => actionEditor(S.actions.find((x) => x.id === b.dataset.edit))));
  $$('[data-del]', el).forEach((b) => (b.onclick = async () => { if (confirm('Delete this action?')) { S = await invoke('delete_action', { id: b.dataset.del }); renderAI(); } }));
  // drag to reorder
  let drag;
  $$('.act-row', el).forEach((r) => {
    r.ondragstart = () => (drag = r);
    r.ondragover = (e) => { e.preventDefault(); const list = r.parentNode; const after = e.offsetY > r.offsetHeight / 2; list.insertBefore(drag, after ? r.nextSibling : r); };
    r.ondragend = () => saveSettings({ action_order: $$('.act-row', el).map((x) => x.dataset.id) }).then((s) => (S = s));
  });
  const debounce = (fn, ms = 500) => { let t; return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); }; };
  $('#styleText').oninput = debounce((e) => saveSettings({ style_text: e.target.value }).then((s) => (S = s)));
  $$('[data-ex]', el).forEach((c) => (c.onclick = () => { const t = $('#styleText'); t.value = (t.value.trim() ? t.value.trim() + '\n' : '') + c.dataset.ex + '.'; t.dispatchEvent(new Event('input')); }));
  $('#ctxText').oninput = debounce((e) => { $('#ctxCount').textContent = `${e.target.value.length} chars`; saveSettings({ prompt_context: e.target.value }).then((s) => (S = s)); });
  $('#ctxFile').onchange = async (e) => { const f = e.target.files[0]; if (!f) return; $('#ctxText').value = await f.text(); $('#ctxText').dispatchEvent(new Event('input')); };
  $('#ctxImprove').onclick = async () => {
    const t = $('#ctxText'); if (!t.value.trim()) return toast('Write a prompt first, then click Improve.');
    $('#ctxImprove').disabled = true;
    try { t.value = await invoke('improve_text', { text: t.value, kind: 'prompt' }); t.dispatchEvent(new Event('input')); } catch (e) { toast('Failed to improve prompt', String(e), true); }
    $('#ctxImprove').disabled = false;
  };
}

export function orderedActions() {
  const order = S.action_order || [];
  return [...S.actions].sort((a, b) => {
    const ia = order.indexOf(a.id), ib = order.indexOf(b.id);
    if (ia !== -1 || ib !== -1) return (ia === -1 ? 999 : ia) - (ib === -1 ? 999 : ib);
    return (b.favorite - a.favorite);
  });
}

function actionEditor(a = null) {
  const starters = [
    ['Make it funny', 'Rewrite the text to be witty and playful while keeping the meaning.'],
    ['To bullet points', 'Turn the text into short bullet points. Keep every fact, drop the filler.'],
    ['Polite reply', 'Write a short, polite reply to this message. Keep my tone friendly and direct.'],
    ['Explain simply', 'Explain this in plain language a smart 12-year-old would follow. No jargon.'],
    ['Tweet it', 'Condense the text into one post under 280 characters. No hashtags.'],
  ];
  const isNew = !a;
  a = a ? { ...a } : { id: '', name: '', prompt: '', icon: '✨', builtin: false, hidden: false, favorite: false, output: 'replace' };
  openModal(`<h2 style="margin-top:0">${isNew ? 'New action' : 'Edit action'}</h2>
    <p class="muted">${isNew ? 'Runs on the text you select. Pick a starter or write your own.' : 'Changes apply the next time you run it.'}</p>
    ${isNew ? `<div class="row" style="flex-wrap:wrap;margin-bottom:12px">${starters.map(([n], i) => `<span class="chip" data-st="${i}">${n}</span>`).join('')}</div>` : ''}
    <div class="row"><input class="input" id="aIcon" value="${esc(a.icon)}" style="width:60px;text-align:center" /><input class="input grow" id="aName" placeholder="Name, e.g. Make it funny" value="${esc(a.name)}" /></div>
    <textarea class="input" id="aPrompt" style="margin-top:10px;min-height:110px" placeholder="Write it the way you would brief a colleague. The selected text is added for you.">${esc(a.prompt)}</textarea>
    <div class="row" style="margin-top:10px"><span class="muted small">Result:</span><div class="seg" id="aOut"><button data-v="replace">Replaces the text</button><button data-v="add">Adds new text</button></div>
      <span class="spacer"></span><button class="btn sm" id="aImprove">${icon('wand', 13)} Improve</button></div>
    <div class="row" style="margin-top:18px"><span class="faint small">Ctrl+Enter to save</span><span class="spacer"></span><button class="btn" id="aCancel">Cancel</button><button class="btn primary" id="aSave">${isNew ? 'Create action' : 'Save changes'}</button></div>`);
  const seg = () => $$('#aOut button').forEach((b) => b.classList.toggle('on', b.dataset.v === a.output));
  seg();
  $$('#aOut button').forEach((b) => (b.onclick = () => { a.output = b.dataset.v; seg(); }));
  $$('[data-st]').forEach((c) => (c.onclick = () => { const [n, p] = starters[c.dataset.st]; $('#aName').value = n; $('#aPrompt').value = p; }));
  $('#aImprove').onclick = async () => { const t = $('#aPrompt'); if (!t.value.trim()) return; t.value = await invoke('improve_text', { text: t.value, kind: 'instruction' }).catch((e) => (toast('Failed', String(e), true), t.value)); };
  $('#aCancel').onclick = closeModal;
  const save = async () => {
    Object.assign(a, { name: $('#aName').value.trim(), prompt: $('#aPrompt').value.trim(), icon: $('#aIcon').value.trim() || '✨' });
    try { S = await invoke('save_action', { action: a }); closeModal(); toast('Saved to your actions'); renderAI(); } catch (e) { toast("Couldn't save", String(e), true); }
  };
  $('#aSave').onclick = save;
  $('#modalCard').onkeydown = (e) => e.ctrlKey && e.key === 'Enter' && save();
  $('#aName').focus();
}

// ============================================================ SETTINGS
function sw(key) {
  return `<label class="switch"><input type="checkbox" data-sw="${key}" ${S[key] ? 'checked' : ''}><span></span></label>`;
}
function bindSwitches(el) {
  $$('[data-sw]', el).forEach((i) => (i.onchange = async () => (S = await saveSettings({ [i.dataset.sw]: i.checked }))));
  $$('[data-sel]', el).forEach((i) => (i.onchange = async () => (S = await saveSettings({ [i.dataset.sel]: isNaN(+i.value) || i.value === '' ? i.value : +i.value }))));
  $$('.seg[data-seg]', el).forEach((seg) => $$('button', seg).forEach((b) => {
    b.classList.toggle('on', String(S[seg.dataset.seg]) === b.dataset.v);
    b.onclick = async () => { S = await saveSettings({ [seg.dataset.seg]: isNaN(+b.dataset.v) ? b.dataset.v : +b.dataset.v }); render(current); };
  }));
}
const row = (title, desc, control) => `<div class="set-row"><div class="s-text"><b>${title}</b><span>${desc}</span></div>${control}</div>`;
const select = (key, opts) => `<select class="input" data-sel="${key}">${opts.map(([v, l]) => `<option value="${esc(v)}" ${String(S[key]) === String(v) ? 'selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
const seg = (key, opts) => `<div class="seg" data-seg="${key}">${opts.map(([v, l]) => `<button data-v="${v}">${l}</button>`).join('')}</div>`;
const SHORTCUT_NAMES = { actions: 'Quick Actions', launcher: 'Launcher', clipboard: 'Clipboard History', voice: 'Voice Input', ocr: 'Text from Screen', undo: 'Undo last change', reply: 'Reply from screen', summarize: 'Summarize page', show_app: 'Open Keeyz' };

// ============================================================ SNAP (window snapping, was Snapperr)
// A tiny screen with the part the window goes to highlighted.
const mini = (x, w, cols = 0) => `<span class="snap-mini">${cols ? Array.from({ length: cols }, (_, i) => `<i style="left:${(i / cols) * 100}%;width:${100 / cols}%" class="${i === 1 ? 'on' : 'col'}"></i>`).join('') : `<i class="on" style="left:${x * 100}%;width:${w * 100}%"></i>`}</span>`;
// [key id, name, x, width] – every key can be changed by clicking it
const SNAP_LAYOUTS = [
  ['left_third', 'Left third', 0, 1 / 3], ['center_third', 'Center third', 1 / 3, 1 / 3], ['right_third', 'Right third', 2 / 3, 1 / 3],
  ['left_half', 'Left half', 0, 1 / 2], ['right_half', 'Right half', 1 / 2, 1 / 2], ['full', 'Full screen', 0, 1],
];
const SNAP_NUDGE = [
  ['move_left', 'Move left'], ['move_right', 'Move right'], ['move_up', 'Move up'], ['move_down', 'Move down'],
  ['narrower', 'Narrower'], ['wider', 'Wider'], ['shorter', 'Shorter'], ['taller', 'Taller'],
];
const KEY_NAMES = { ArrowLeft: '←', ArrowRight: '→', ArrowUp: '↑', ArrowDown: '↓', Escape: 'Esc' };
const snapKeys = (accel) => (accel || '').split('+').filter(Boolean).map((k) => `<kbd>${esc(KEY_NAMES[k] || k)}</kbd>`).join('');
const snapKey = (id) => `<button class="snap-key" data-snapkey="${id}" title="Click, then press the new keys">${snapKeys(S.snap_keys?.[id])}</button>`;
async function renderSnap() {
  const el = $('#view-snap');
  const [st, old] = await Promise.all([invoke('snap_state'), invoke('snapperr_status').catch(() => ({}))]);
  const colMods = S.snap_keys?.columns || 'Ctrl';
  el.innerHTML = `<div class="snap-hero">
      <span class="snap-mark">${icon('layout', 26)}</span>
      <div class="grow"><h1 style="margin:0 0 4px">Snap windows with your keyboard</h1>
        <div class="muted">Double-tap <kbd>Ctrl</kbd> to turn on snap mode, then put the window where you want it. The keys only work while snap mode is on, so other apps keep them.</div></div>
      <label class="snap-live ${st.on ? 'on' : ''}" id="snapLive" title="Keep snap mode on until you switch it off"><span class="dot"></span><span id="snapLiveText">${st.on ? 'Snap mode on' : 'Snap mode off'}</span><span class="switch sm"><input type="checkbox" id="snapSticky" ${st.on ? 'checked' : ''}><span></span></span></label>
    </div>
    ${old.installed || old.running ? `<div class="snap-note"><span>${icon('info', 15)}</span><div class="grow"><b>The old Snapperr app is still ${old.running ? 'running' : 'installed'}.</b><div class="muted small">Snap is now part of Keeyz. With both running, every Ctrl double-tap happens twice.</div></div>
      ${old.running ? '<button class="btn sm" id="oldClose">Close it</button>' : ''}${old.installed ? '<button class="btn sm primary" id="oldUninstall">Uninstall it</button>' : ''}</div>` : ''}
    <div class="snap-steps">
      <div class="snap-step"><span class="snap-taps"><kbd>Ctrl</kbd><kbd>Ctrl</kbd></span><div><b>Double-tap Ctrl</b><div class="muted small">Snap mode on for ${S.snap_seconds} seconds. A soft sound plays and a small counter shows at the top of the screen.</div></div></div>
      <div class="snap-step"><span class="snap-taps"><kbd>Ctrl</kbd><kbd>Ctrl</kbd><kbd>Ctrl</kbd></span><div><b>Triple-tap Ctrl</b><div class="muted small">Pick 2 to 9 columns, then ${snapKeys(colMods)}+<kbd>1</kbd>…<kbd>9</kbd> puts the window in that column.</div></div></div>
      <div class="snap-step"><span class="snap-taps">${snapKeys(S.snap_keys?.off)}</span><div><b>Done</b><div class="muted small">This key or another double-tap ends it. Every key press keeps it on a little longer.</div></div></div>
    </div>
    <div class="snap-h"><span>Layouts</span><span class="faint small">Click a key to change it.</span><span class="spacer"></span><button class="btn ghost sm" id="snapReset">Reset keys</button></div>
    <div class="snap-grid">
      ${SNAP_LAYOUTS.map(([id, name, x, w]) => `<div class="snap-card">${mini(x, w)}<b>${name}</b>${snapKey(id)}</div>`).join('')}
      <div class="snap-card">${mini(0, 0, 4)}<b>Columns</b><button class="snap-key" data-snapkey="columns" title="Click, then hold the modifier keys and press any number">${snapKeys(colMods)}<span class="faint small">+ 1…9</span></button></div>
    </div>
    <div class="set-group" style="margin-top:14px"><div class="g-title"><span class="g-ico">${icon('move', 15)}</span>Move and resize <span class="spacer"></span><span class="faint small">20 px per press. Hold to keep going.</span></div>
      <div class="snap-nudge">${SNAP_NUDGE.map(([id, name]) => `<div class="snap-nudge-row"><span>${name}</span>${snapKey(id)}</div>`).join('')}
        <div class="snap-nudge-row"><span>Snap mode off</span>${snapKey('off')}</div></div>
    </div>
    <div class="set-group"><div class="g-title"><span class="g-ico">${icon('layout', 15)}</span>Snap settings</div>
      ${row('Snap windows', 'Double-tap or triple-tap Ctrl to snap windows. Turn off if another app uses Ctrl taps.', sw('snap_enabled'))}
      ${row('Soft sounds', 'A quiet sound when snap mode turns on and off.', sw('snap_sounds'))}
      ${row('Snap mode stays on', 'Seconds after the last key press.', seg('snap_seconds', [[5, '5 s'], [10, '10 s'], [20, '20 s']]))}
      ${row('Try it', 'Turns snap mode on for this window, so you can try the keys right here.', `<button class="btn sm" id="snapTry">${icon('play', 12)} Try it now</button>`)}
    </div>
    <p class="faint small">Snaps to the visible edges of the window, on the screen it is on. Windows doesn't let normal apps move windows of apps running as administrator, so those can't be snapped.</p>`;
  bindSwitches(el);
  $('#snapTry').onclick = () => invoke('snap_start');
  $('#snapSticky').onchange = (e) => {
    invoke('snap_set', { on: e.target.checked });
    if (e.target.checked) toast('Snap mode stays on', 'Ctrl + arrows, Ctrl+L, Ctrl+; and Ctrl+F now snap windows. Switch it off here, in the tray, or double-tap Ctrl.');
  };
  $('#snapReset').onclick = async () => { if (confirm('Put all snap keys back to the defaults?')) { S = await invoke('reset_section', { section: 'snap' }); renderSnap(); } };
  $$('[data-snapkey]', el).forEach((b) => (b.onclick = () => record(b, async (acc) => {
    const id = b.dataset.snapkey;
    // columns keep only the modifiers: "Ctrl+Alt+3" -> "Ctrl+Alt"
    const val = id === 'columns' ? acc.split('+').filter((k) => ['Ctrl', 'Alt', 'Shift'].includes(k)).join('+') || 'Ctrl' : acc;
    const clash = Object.entries(S.snap_keys).find(([k, v]) => k !== id && k !== 'columns' && v === val);
    if (clash) return toast('That key is already used', `It is set for “${clash[0].replace('_', ' ')}”. Pick another one.`, true);
    S = await saveSettings({ snap_keys: { ...S.snap_keys, [id]: val } });
  }, renderSnap, true)));
  $('#oldClose')?.addEventListener('click', async () => { await invoke('snapperr_close'); toast('Snapperr closed'); renderSnap(); });
  $('#oldUninstall')?.addEventListener('click', async () => {
    try { await invoke('snapperr_uninstall'); toast('Snapperr uninstaller opened', 'Confirm there to remove the old app.'); } catch (e) { toast("Couldn't uninstall Snapperr", String(e), true); }
    setTimeout(() => current === 'snap' && renderSnap(), 8000);
  });
}
listen('snap-state', (e) => {
  const live = $('#snapLive');
  if (!live) return;
  const on = !!(e.payload && (e.payload.on ?? e.payload));
  live.classList.toggle('on', on);
  $('#snapLiveText').textContent = on ? 'Snap mode on' : 'Snap mode off';
  if ($('#snapSticky')) $('#snapSticky').checked = on;
});

async function renderSettings() {
  const el = $('#view-settings');
  const info = await invoke('app_info');
  const langOpts = TRANSLATE_LANGS.map((l) => [l, l]);
  const actionOpts = [['none', 'Nothing'], ...S.actions.map((a) => [a.id, a.name])];
  let mics = [['', 'System default']];
  try { mics = mics.concat((await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audioinput' && d.deviceId !== 'default').map((d) => [d.deviceId, d.label || 'Microphone'])); } catch {}
  el.innerHTML = `<h1>Settings</h1><p class="lead">Everything is unlocked. Your AI key stays on this PC (Windows Credential Manager).</p>
    <input class="input settings-search" id="setSearch" placeholder="Search settings…" />
    <div class="set-group"><div class="g-title"><span class="g-ico">${icon('keyboard', 15)}</span>Typing trigger</div>
      ${row('Typed commands', `Run <b>${esc(S.trigger_word)}</b> and commands like <code>/calc</code>, <code>/search</code>, <code>/yt</code>, <code>/maps</code> when you type them, in any app.`, sw('typed_commands'))}
      ${row('Trigger word', 'The word that wakes Keeyz while you type, in any case (Keeyz or keeyz). 1–30 characters, no spaces.', `<input class="input" id="trigWord" value="${esc(S.trigger_word)}" spellcheck="false" />`)}
      ${row('Run on pause', 'No double space needed: it runs about 2 seconds after you stop typing.', sw('instant_run'))}
      ${row('Live translate', 'Type in your language; a double space replaces it with the translation.', sw('live_translate'))}
      ${row('Translate into', 'Target language for Live translate.', select('live_translate_lang', langOpts))}
      ${row('Rewrite the whole text field', 'The trigger rewrites everything in the box, not only the text typed since the last Enter.', sw('rewrite_whole_field'))}
      ${row('Double space runs', 'An action to run on what you just typed when you press space twice. Live translate takes precedence while it is on.', select('double_space_action', actionOpts))}
      ${row('Spaces needed', 'How many spaces in a row run the trigger. Pick three if two happen by accident.', seg('run_spaces', [[2, 'Two'], [3, 'Three']]))}
      <div class="set-row" style="display:block"><b>Your commands</b> <span class="muted small">— e.g. <code>/cost</code> → “Add a cost column in euros”</span>
        <div id="cmdList" style="margin-top:8px">${S.custom_commands.map((c, i) => `<div class="row" style="margin-bottom:6px"><code style="min-width:80px">${esc(c.pattern)}</code><span class="grow muted small">${esc(c.prompt)} <i>(${c.scope === 'after' ? 'on the text typed after it' : 'on the line typed before it'})</i></span><button class="btn ghost sm danger" data-rmcmd="${i}">Remove</button></div>`).join('')}</div>
        <div class="row" style="margin-top:6px"><input class="input" id="cPat" placeholder="/cmd" style="width:110px" /><input class="input grow" id="cPrompt" placeholder="What should happen to the text" />
          <select class="input" id="cScope" style="width:170px"><option value="before">on text before it</option><option value="after">on text after it</option></select><button class="btn" id="cAdd">${icon('plus', 14)} Add</button></div></div>
      <div class="set-row"><span class="spacer"></span><button class="btn sm" data-reset="trigger">Reset defaults</button></div>
    </div>
    <div class="set-group"><div class="g-title"><span class="g-ico">${icon('command', 15)}</span>Shortcuts <span class="spacer"></span><span class="faint small">Click a hotkey to set it, or × to turn it off.</span></div>
      ${Object.entries(SHORTCUT_NAMES).map(([id, name]) => row(name, '', `<span class="hk" data-hk="${id}">${kbd(S.shortcuts[id])}</span><button class="btn ghost icon sm" data-hkoff="${id}" title="Turn off">${icon('x', 14)}</button>`)).join('')}
      <div class="set-row" style="display:block"><b>Yours</b> <span class="muted small">— one-key shortcuts for an action, a translation, or a workflow</span>
        ${S.custom_shortcuts.map((c, i) => `<div class="row" style="margin-top:6px"><span class="grow">${esc(customLabel(c))}</span><span class="hk" data-chk="${i}">${kbd(c.accelerator)}</span><button class="btn ghost icon sm danger" data-crm="${i}">${icon('x', 14)}</button></div>`).join('')}
        <div class="row" style="margin-top:8px"><select class="input" id="nsKind" style="width:140px"><option value="action">Action</option><option value="translate">Translate to</option><option value="workflow">Workflow</option></select>
          <select class="input grow" id="nsTarget"></select><button class="btn" id="nsAdd">New shortcut</button></div></div>
      <div class="set-row"><span class="spacer"></span><button class="btn sm" data-reset="shortcuts">Reset all</button></div>
    </div>
    <div class="set-group"><div class="g-title"><span class="g-ico">${icon('palette', 15)}</span>Appearance</div>
      ${row('Theme', 'Applies to every Keeyz window.', select('theme', [['system', 'System'], ['light', 'Light'], ['dark', 'Dark'], ['midnight', 'Midnight (OLED)'], ['ocean', 'Ocean'], ['forest', 'Forest'], ['sepia', 'Sepia']]))}
      ${row('Text size', 'Scales the whole interface, not just the text.', seg('text_size', [['small', 'Small'], ['default', 'Default'], ['large', 'Large'], ['xlarge', 'Extra large']]))}
      ${row('Default translate language', 'Used by Translate when you don\'t pick one.', select('default_translate_lang', langOpts))}
    </div>
    <div class="set-group"><div class="g-title"><span class="g-ico">${icon('sliders', 15)}</span>Behavior</div>
      ${row('Show results in a window', 'Quick Actions shows the answer for review first. Turn off to paste it straight into your app.', sw('output_window'))}
      ${row('Toolbar on selected text', 'Select text in any app to get the right buttons for it next to it: Fix and Shorten for your own text, Explain or Define for a word, Summarize for long text. Translate is always there.', sw('selection_toolbar'))}
      ${row('Closing the window', 'Keeyz keeps running in the tray so your shortcuts work everywhere.', seg('close_behavior', [['tray', 'Keep in tray'], ['quit', 'Quit']]))}
      ${row('Start on boot', 'Launch Keeyz when your computer starts.', sw('start_on_boot'))}
      ${row('Notifications', 'Show system notifications (results, reminders, errors).', sw('notifications'))}
      ${row('Sound effects', 'Play a short sound when an action finishes.', sw('sounds'))}
      ${row('Hide when clicking elsewhere', 'Send this window to the tray whenever another app takes focus.', sw('hide_on_blur'))}
      ${row('Notify when sent to tray', 'Show a notification every time the window hides.', sw('notify_on_tray'))}
      ${row('Always on top', 'Keep this window above other apps.', sw('always_on_top'))}
    </div>
    <div class="set-group"><div class="g-title"><span class="g-ico">${icon('mic', 15)}</span>Voice</div>
      ${row('Edit voice text before pasting', 'Off: your words are pasted as soon as you stop. On: you see the text first and can change it.', sw('voice_review'))}
      ${row('Stop voice when I stop talking', 'Finishes by itself after a short silence. Otherwise press the voice shortcut again.', sw('voice_auto_stop'))}
      ${row('Voice language', 'The language you speak when you dictate.', select('voice_lang', VOICE_LANGS))}
      ${row('Microphone', 'Which microphone voice input listens to.', select('voice_mic', mics))}
    </div>
    <div class="set-group"><div class="g-title"><span class="g-ico">${icon('info', 15)}</span>About</div>
      ${row('Updates', `You have version <b>${esc(info.version)}</b>. <span id="updText">Keeyz checks for new versions by itself.</span>`, `<div class="ctl"><button class="btn sm" id="updCheck">${icon('refresh', 13)} Check for updates</button><button class="btn sm primary hidden" id="updNow">Update now</button></div>`)}
      ${row('Keeyz', `Data in <span class="sel mono small">${esc(info.data_dir)}</span>`, '<button class="btn sm" id="runTour">Run tour</button>')}
      ${row('Delete everything', 'Removes your API keys, history, clipboard history, stats and settings from this PC.', '<button class="btn sm danger" id="delAll">Delete…</button>')}
      ${row('Quit Keeyz', 'Shortcuts stop until you open it again.', '<button class="btn sm" id="quit">Quit</button>')}
    </div>`;
  bindSwitches(el);
  $('#setSearch').oninput = (e) => { const q = e.target.value.toLowerCase(); $$('.set-row', el).forEach((r) => r.classList.toggle('hidden', !!q && !r.textContent.toLowerCase().includes(q))); };
  $('#trigWord').onchange = async (e) => {
    const v = e.target.value.trim();
    if (v.length < 1) return toast('Trigger word cannot be empty', '', true);
    if (v.length > 30) return toast('Trigger word must be 30 characters or less', '', true);
    if (/\s/.test(v)) return toast('Trigger word cannot contain spaces', '', true);
    S = await saveSettings({ trigger_word: v }); toast('Trigger word saved', `Type: your text ${v} make it formal`);
  };
  $$('[data-reset]', el).forEach((b) => (b.onclick = async () => { if (confirm('Reset to defaults?')) { S = await invoke('reset_section', { section: b.dataset.reset }); renderSettings(); } }));
  $('#cAdd').onclick = async () => {
    const pattern = $('#cPat').value.trim(), p = $('#cPrompt').value.trim();
    if (pattern.length < 2 || pattern.length > 49) return toast('A command needs 2 to 49 characters.', '', true);
    if (S.custom_commands.some((c) => c.pattern === pattern)) return toast('That command already exists.', '', true);
    if (!p) return toast('Add what should happen to the text.', '', true);
    S = await saveSettings({ custom_commands: [...S.custom_commands, { pattern, prompt: p, scope: $('#cScope').value }] }); renderSettings();
  };
  $$('[data-rmcmd]', el).forEach((b) => (b.onclick = async () => { const c = [...S.custom_commands]; c.splice(+b.dataset.rmcmd, 1); S = await saveSettings({ custom_commands: c }); renderSettings(); }));
  // shortcut recording
  $$('[data-hk]', el).forEach((hk) => (hk.onclick = () => record(hk, async (acc) => { S = await saveSettings({ shortcuts: { ...S.shortcuts, [hk.dataset.hk]: acc } }); })));
  $$('[data-hkoff]', el).forEach((b) => (b.onclick = async () => { S = await saveSettings({ shortcuts: { ...S.shortcuts, [b.dataset.hkoff]: '' } }); renderSettings(); }));
  $$('[data-chk]', el).forEach((hk) => (hk.onclick = () => record(hk, async (acc) => { const c = structuredClone(S.custom_shortcuts); c[+hk.dataset.chk].accelerator = acc; S = await saveSettings({ custom_shortcuts: c }); })));
  $$('[data-crm]', el).forEach((b) => (b.onclick = async () => { const c = [...S.custom_shortcuts]; c.splice(+b.dataset.crm, 1); S = await saveSettings({ custom_shortcuts: c }); renderSettings(); }));
  const fillTargets = () => {
    const k = $('#nsKind').value;
    const opts = k === 'action' ? S.actions.map((a) => [a.id, a.name]) : k === 'workflow' ? S.workflows.map((w) => [w.id, w.name]) : TRANSLATE_LANGS.map((l) => [l, l]);
    $('#nsTarget').innerHTML = opts.map(([v, l]) => `<option value="${esc(v)}">${esc(l)}</option>`).join('');
  };
  $('#nsKind').onchange = fillTargets; fillTargets();
  $('#nsAdd').onclick = async () => {
    const c = { id: Math.random().toString(36).slice(2, 9), kind: $('#nsKind').value, target: $('#nsTarget').value, accelerator: '' };
    S = await saveSettings({ custom_shortcuts: [...S.custom_shortcuts, c] }); await renderSettings();
    const last = $$('[data-chk]').pop(); last?.click();
  };
  $('#runTour').onclick = () => welcome(true);
  $('#updCheck').onclick = () => checkUpdate(true);
  $('#updNow').onclick = installUpdate;
  if (pendingUpdate) showUpdate(pendingUpdate);
  $('#quit').onclick = () => invoke('quit_app');
  $('#delAll').onclick = async () => {
    if (!confirm('Delete your API keys, history, clipboard history, stats and settings from this PC? This cannot be undone.')) return;
    await invoke('delete_everything'); S = await getSettings(true); toast('Everything deleted'); renderSettings();
  };
}
function customLabel(c) {
  if (c.kind === 'translate') return `Translate to ${c.target}`;
  if (c.kind === 'workflow') return `Workflow: ${S.workflows.find((w) => w.id === c.target)?.name || c.target}`;
  return S.actions.find((a) => a.id === c.target)?.name || c.target;
}
/** Record a key combination. `anyKey`: also accept keys without Ctrl/Alt/Shift (snap keys). */
function record(el, onDone, after = renderSettings, anyKey = false) {
  el.classList.add('rec');
  el.innerHTML = '<span class="muted">Press keys…</span>';
  invoke('suspend_shortcuts', { suspend: true });
  const handler = async (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape' && !(anyKey && el.dataset.snapkey === 'off')) return finish();
    let acc = accelFromEvent(e);
    if (!acc && anyKey && !/^(Control|Shift|Alt|Meta|OS)/.test(e.code)) acc = e.code === 'Escape' ? 'Escape' : e.code.replace(/^Key/, '').replace(/^Digit/, '');
    if (!acc) return;
    await onDone(acc);
    finish();
  };
  const finish = () => { window.removeEventListener('keydown', handler, true); invoke('suspend_shortcuts', { suspend: false }); after(); };
  window.addEventListener('keydown', handler, true);
}

// ============================================================ PROVIDER
// Key status on each provider card: is the key really working, or what is wrong.
const keyHealth = {}; // provider -> { state: 'ok'|'warn'|'bad'|'checking', label, detail }
function keyBadge(id, keys) {
  if (id === 'auto') return '<span class="badge">Recommended</span>';
  const has = keys[id]?.set || (id === 'custom' && S.provider === 'custom');
  if (!has) return id === 'custom' ? '<span class="badge neutral">No key needed</span>' : '<span class="faint">No key yet</span>';
  return `<span class="kh" data-kh="${id}">${khHtml(keyHealth[id] || { state: 'checking' })}</span>`;
}
function khHtml(h) {
  if (h.state === 'checking') return '<span class="kh-pill checking"><span class="spinner"></span>Checking key…</span>';
  const ico = h.state === 'ok' ? icon('check', 11, 2.4) : h.state === 'warn' ? icon('info', 11, 2) : icon('x', 11, 2.4);
  return `<span class="kh-pill ${h.state}" title="${esc(h.detail || '')}">${ico}${esc(h.label)}</span>`;
}
// Check every saved key (results are kept 10 min by Keeyz; force = check again now).
async function checkKeys(keys, force = false) {
  const ids = PROVIDERS.map((p) => p[0]).filter((id) => id !== 'auto' && (keys[id]?.set || (id === 'custom' && S.provider === 'custom')));
  await Promise.all(ids.map(async (id) => {
    if (force || !keyHealth[id]) {
      keyHealth[id] = { state: 'checking' };
      $$(`[data-kh="${id}"]`).forEach((el) => (el.innerHTML = khHtml(keyHealth[id])));
    }
    try { keyHealth[id] = await invoke('key_health', { provider: id, force }); } catch (e) { keyHealth[id] = { state: 'bad', label: 'Not working', detail: String(e) }; }
    $$(`[data-kh="${id}"]`).forEach((el) => (el.innerHTML = khHtml(keyHealth[id])));
  }));
}

const PROVIDERS = [
  ['auto', 'Auto · fastest', 'Uses the quickest model that works with your keys. Re-checks by itself.', []],
  ['openai', 'OpenAI', 'GPT models. Key starts with <code>sk-</code>', ['gpt-4o-mini', 'gpt-4o', 'gpt-4.1-mini', 'gpt-4.1', 'gpt-5-mini', 'gpt-5']],
  ['anthropic', 'Anthropic', 'Claude models. Key starts with <code>sk-ant-</code>', ['claude-haiku-4-5', 'claude-sonnet-4-5', 'claude-opus-4-1']],
  ['gemini', 'Google Gemini', 'Free tier. Key starts with <code>AIza</code> or <code>AQ.</code>', ['gemini-flash-latest', 'gemini-flash-lite-latest', 'gemini-pro-latest']],
  ['openrouter', 'OpenRouter', 'Hundreds of models, one key. Starts with <code>sk-or-</code>', ['openai/gpt-4o-mini', 'anthropic/claude-haiku-4.5', 'google/gemini-flash-latest', 'nvidia/nemotron-3.5-lightning:free', 'inclusionai/ling-3.2-flash:free']],
  ['deepseek', 'DeepSeek', 'Low-cost models. Key from platform.deepseek.com, starts with <code>sk-</code>', ['deepseek-chat', 'deepseek-reasoner']],
  ['custom', 'Local / custom', 'Ollama, LM Studio or any OpenAI-compatible URL', ['llama3.2', 'qwen3:8b', 'mistral']],
];
let modelCache = {};
const testState = {}; // provider -> { ok, msg }
const PDOT = { auto: '#2f6f64', openai: '#10a37f', anthropic: '#d97757', gemini: '#5b7fd1', openrouter: '#6366f1', deepseek: '#4d6bfe', custom: '#64748b' };
const WF_ICON = { meeting_notes: 'fileText', email_responder: 'mail', translate_polish: 'globe' };
// Things people paste into the model box that are not model ids (e.g. a Google Cloud project name).
const NOT_A_MODEL = /^(gen-lang-client-|projects\/|AIza|AQ\.|sk-)/i;

async function renderProvider() {
  const el = $('#view-provider');
  // Clean up a wrong "custom model id" left from older versions (e.g. a Google Cloud project name).
  if (S.custom_model) {
    const old = S.custom_model;
    const keep = !NOT_A_MODEL.test(old);
    S = await saveSettings({ custom_model: '', models: keep ? { ...S.models, [S.provider]: old } : S.models });
    if (!keep) toast('Fixed your model setting', `“${old}” is a project or key, not a model. Using ${S.models[S.provider]} instead.`);
  }
  const keys = await invoke('key_status');
  const p = PROVIDERS.find((x) => x[0] === S.provider) || PROVIDERS[0];
  const pid = p[0];
  if (pid === 'auto') return renderAuto(el, keys);
  const k = keys[pid] || {};
  const models = [...new Set([...p[3], ...(modelCache[pid] || []), S.models[pid]].filter(Boolean))];
  const st = testState[pid];
  const pill = st ? (st.ok ? `<span class="pill ok">${icon('check', 12)} Connected</span>` : `<span class="pill err">${icon('x', 12)} Not working</span>`)
    : k.set || pid === 'custom' ? '<span class="pill">Not tested yet</span>' : '<span class="pill">No key yet</span>';

  el.innerHTML = `<h1>AI Provider</h1>
    <p class="lead">Keeyz runs on your own API key: unlimited, no credits. Requests go straight from this PC to the provider. If your model is busy, out of credit or unavailable, Keeyz switches by itself to the next fast model you have a key for (never for a local model).</p>
    <div class="row" style="margin:-10px 0 12px"><span class="faint small">Each saved key is tested with a tiny request (costs next to nothing).</span><span class="spacer"></span><button class="btn ghost sm" id="recheckKeys">${icon('refresh', 13)} Check keys again</button></div>
    <div class="prov-grid">${PROVIDERS.map(([id, name, tag]) => `
      <button class="prov ${id === pid ? 'on' : ''}" data-p="${id}">
        <span class="p-head"><span class="p-dot" style="background:${PDOT[id]}"></span><span class="p-name">${name}</span>${id === pid ? `<span class="p-check">${icon('check', 12, 2.4)}</span>` : ''}</span>
        <span class="p-tag">${tag}</span>
        <span class="p-foot">${keyBadge(id, keys)}</span>
      </button>`).join('')}</div>

    <div class="set-group" style="margin-top:24px">
      <div class="g-title"><span class="p-dot" style="background:${PDOT[pid]};width:12px;height:12px"></span>${esc(p[1])} setup<span class="spacer"></span>${pill}</div>
      ${pid === 'custom' ? row('Endpoint URL', 'Your OpenAI-compatible server. Ollama: <code>http://localhost:11434/v1</code> · LM Studio: <code>http://localhost:1234/v1</code>',
        `<div class="ctl"><input class="input" id="endpoint" value="${esc(S.endpoint_url)}" spellcheck="false" /></div>`) : ''}
      ${row(pid === 'custom' ? 'API key <span class="faint">(optional)</span>' : 'API key',
        k.set ? `Saved on this PC: <span class="mono">${esc(k.masked)}</span>` : 'Paste it once. It is kept in Windows Credential Manager, never in a file.',
        `<div class="ctl"><input class="input" type="password" id="apiKey" placeholder="${k.set ? 'Paste a new key to replace' : 'Paste your key'}" spellcheck="false" autocomplete="off" />${k.set ? `<button class="btn ghost icon" id="clearKey" title="Remove key">${icon('trash', 15)}</button>` : ''}</div>`)}
      ${row('Model', 'Pick one, or type the exact model id. Smaller models are faster and cheaper.',
        `<div class="ctl"><input class="input" id="model" list="modelList" value="${esc(S.models[pid] || '')}" spellcheck="false" /><datalist id="modelList">${models.map((m) => `<option value="${esc(m)}"></option>`).join('')}</datalist><button class="btn ghost icon" id="loadModels" title="Load all models from ${esc(p[1])}">${icon('refresh', 15)}</button></div>`)}
      ${pid === 'custom' ? row('Thinking', 'Lets reasoning models (Qwen3, DeepSeek R1) think before they answer. Better for hard questions, slower.', sw('thinking')) : ''}
      ${row('Test connection', 'Sends a tiny request to check the key and model.', `<div class="ctl"><button class="btn" id="test">${icon('play', 12)} Test connection</button></div>`)}
      <div class="callout-row ${st ? '' : 'hidden'}" id="testRow"><div class="callout ${st?.ok ? 'ok' : 'err'}" id="testBox">${st ? calloutHtml(st) : ''}</div></div>
    </div>
    <p class="faint small foot-note">${icon('mic', 13)} Voice typing needs an OpenAI or Gemini key (or a local Whisper server). Anthropic keys can't transcribe audio.</p>`;

  bindSwitches(el);
  checkKeys(keys);
  $('#recheckKeys').onclick = () => checkKeys(keys, true);
  $$('.prov', el).forEach((c) => (c.onclick = async () => { S = await saveSettings({ provider: c.dataset.p }); renderProvider(); }));
  $('#apiKey').onchange = async (e) => {
    const v = e.target.value.trim();
    if (!v) return;
    if (/^(gen-lang-client-|projects\/)/i.test(v)) return showTest({ ok: false, msg: 'That is a Google Cloud project name, not an API key. Copy the key from aistudio.google.com/apikey.' });
    try { await invoke('set_api_key', { provider: pid, key: v }); } catch (err) { return toast("Couldn't save key", String(err), true); }
    delete keyHealth[pid];
    toast('Key saved', 'Checking it now…');
    await renderProvider();
    runTest();
  };
  $('#clearKey')?.addEventListener('click', async () => { if (!confirm('Remove the saved key for this provider?')) return; await invoke('set_api_key', { provider: pid, key: '' }); delete testState[pid]; delete keyHealth[pid]; renderProvider(); });
  $('#model').onchange = async (e) => {
    const v = e.target.value.trim();
    if (!v) return;
    if (NOT_A_MODEL.test(v)) { e.target.value = S.models[pid]; return showTest({ ok: false, msg: `“${v}” is not a model id. Pick a model from the list, for example ${p[3][0]}.` }); }
    S = await saveSettings({ models: { ...S.models, [pid]: v }, custom_model: '' });
    delete testState[pid];
    toast('Model saved', v);
  };
  $('#endpoint')?.addEventListener('change', async (e) => (S = await saveSettings({ endpoint_url: e.target.value.trim() })));
  $('#loadModels').onclick = async () => {
    const b = $('#loadModels'); b.innerHTML = '<div class="spinner"></div>';
    try { modelCache[pid] = await invoke('list_models'); toast(`${modelCache[pid].length} models loaded`, 'Click the Model box to pick one.'); renderProvider(); }
    catch (e) { toast("Couldn't load models", shortErr(e), true); b.innerHTML = icon('refresh', 15); }
  };
  $('#test').onclick = runTest;

  function showTest(state) {
    testState[pid] = state;
    $('#testRow').classList.remove('hidden');
    $('#testBox').className = `callout ${state.ok ? 'ok' : state.busy ? 'busy' : 'err'}`;
    $('#testBox').innerHTML = calloutHtml(state);
  }
  async function runTest() {
    showTest({ busy: true, msg: pid === 'custom' ? 'Testing… a local model can take up to a minute to load the first time.' : 'Testing connection…' });
    $('#test').disabled = true;
    try { await invoke('test_connection'); testState[pid] = { ok: true, msg: `Your key works with ${S.models[pid]}. You're ready to go.` }; }
    catch (e) {
      const err = String(e);
      // Retired / unknown model: pick a current one from the provider's live list and try again.
      // OpenRouter key with a $0 limit (or a busy free model): find a free model that answers.
      if (pid === 'openrouter' && /spending limit|free model is busy/i.test(err)) {
        const ok = await tryFreeModels().catch(() => null);
        if (ok) { testState[pid] = { ok: true, msg: `Your key has no paid budget, so Keeyz switched to the free model ${ok}. You're ready to go. (Raise the key's limit at openrouter.ai/settings/keys to use paid models.)` }; return renderProvider(); }
      }
      const better = /Model not found/i.test(err) ? await pickModel(pid).catch(() => null) : null;
      if (better && better !== S.models[pid]) {
        const old = S.models[pid];
        S = await saveSettings({ models: { ...S.models, [pid]: better } });
        try { await invoke('test_connection'); testState[pid] = { ok: true, msg: `“${old}” is retired, so Keeyz switched to ${better}. Your key works. You're ready to go.` }; }
        catch (e2) { testState[pid] = { ok: false, msg: shortErr(e2) }; }
      } else testState[pid] = { ok: false, msg: shortErr(e) };
    }
    renderProvider();
  }
}
// Auto · fastest: shows the speed ranking and lets the user re-check.
async function renderAuto(el, keys) {
  const st = await invoke('fast_status');
  const r = st.ranking || { list: [] };
  const withKey = ['gemini', 'openrouter', 'openai', 'anthropic', 'deepseek'].filter((x) => keys[x]?.set);
  const ok = r.list.filter((c) => c.ok);
  const when = r.updated ? new Date(r.updated).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' }) : 'never';
  const name = (pid) => PROVIDERS.find((x) => x[0] === pid)?.[1] || pid;
  el.innerHTML = `<h1>AI Provider</h1>
    <p class="lead">Keeyz runs on your own API keys: unlimited, no credits. Requests go straight from this PC to the provider.</p>
    <div class="prov-grid">${provCards(keys)}</div>
    <div class="set-group" style="margin-top:24px">
      <div class="g-title"><span class="p-dot" style="background:${PDOT.auto};width:10px;height:10px"></span>Auto · fastest<span class="spacer"></span>
        ${st.running ? '<span class="pill"><span class="spinner" style="width:11px;height:11px"></span>&nbsp;Checking models…</span>' : ok.length ? `<span class="pill ok">${icon('check', 12)} ${ok.length} fast models ready</span>` : '<span class="pill">Not checked yet</span>'}</div>
      <div class="set-row"><div class="s-text"><b>How it works</b><span>Keeyz reads the live model list of every provider you added a key for (${withKey.length ? withKey.map(name).join(', ') : 'none yet'}), tests the fast and free ones on a small real job, and always uses the quickest model that answered correctly. If one is slow or busy, the next one takes over. It re-checks every 12 hours, so new models are picked up without an update.</span></div></div>
      <div class="set-row"><div class="s-text"><b>Last check</b><span>${esc(when)}${st.last ? ` · last answer came from <b>${esc(st.last)}</b>` : ''}</span></div>
        <div class="ctl"><button class="btn" id="findFast" ${st.running || !withKey.length ? 'disabled' : ''}>${icon('refresh', 13)} Find fastest now</button></div></div>
      ${r.list.length ? `<div class="rank">${r.list.map((c, i) => `<div class="rank-row ${c.ok ? '' : 'off'}">
          <span class="r-n">${c.ok ? i + 1 : '–'}</span>
          <span class="r-m"><b>${esc(c.model)}</b><span class="faint"> · ${esc(name(c.provider))}</span></span>
          <span class="r-s">${c.ok ? `${(c.ms / 1000).toFixed(1)} s` : `<span class="faint" title="${esc(c.note)}">${esc(c.note.length > 34 ? c.note.slice(0, 34) + '…' : c.note || 'not working')}</span>`}</span>
        </div>`).join('')}</div>` : ''}
    </div>
    ${withKey.length ? '' : '<p class="faint small foot-note">Add a key first: pick Google Gemini above (it has a free tier), paste your key, then come back to Auto.</p>'}`;
  bindProvCards(el);
  checkKeys(keys);
  $('#findFast')?.addEventListener('click', async () => {
    $('#findFast').disabled = true;
    $('#findFast').innerHTML = '<div class="spinner"></div> Checking… (about 15 s)';
    try { await invoke('find_fastest'); } catch (e) { toast("Couldn't check models", String(e), true); }
    renderProvider();
  });
}
listen('fast-updated', () => current === 'provider' && S.provider === 'auto' && renderProvider());
const provCards = (keys) => PROVIDERS.map(([id, name, tag]) => `
      <button class="prov ${id === S.provider ? 'on' : ''}" data-p="${id}">
        <span class="p-head"><span class="p-dot" style="background:${PDOT[id]}"></span><span class="p-name">${name}</span>${id === S.provider ? `<span class="p-check">${icon('check', 12, 2.4)}</span>` : ''}</span>
        <span class="p-tag">${tag}</span>
        <span class="p-foot">${keyBadge(id, keys)}</span>
      </button>`).join('');
const bindProvCards = (el) => $$('.prov', el).forEach((c) => (c.onclick = async () => { S = await saveSettings({ provider: c.dataset.p }); renderProvider(); }));

// OpenRouter: test free models one by one and keep the first that answers.
async function tryFreeModels() {
  const list = (await invoke('list_models')).filter((m) => m.endsWith(':free') && !/(vl|omni|code|reasoning|nano|1\.7b)/i.test(m));
  modelCache.openrouter = list;
  const prefer = ['nvidia/nemotron-3.5-lightning:free', 'inclusionai/ling-3.2-flash:free', 'baidu/ernie-5.5-flash-preview:free'];
  const order = [...prefer.filter((m) => list.includes(m)), ...list.filter((m) => !prefer.includes(m))].slice(0, 6);
  for (const m of order) {
    S = await saveSettings({ models: { ...S.models, openrouter: m } });
    try { await invoke('test_connection'); return m; } catch {}
  }
  return null;
}
// Choose a sensible current model from the provider's live list.
async function pickModel(pid) {
  const list = await invoke('list_models');
  modelCache[pid] = list;
  const ok = (m) => !/(tts|image|audio|realtime|transcribe|embed|search|robotics|computer-use|preview-tts|vision|instruct$)/i.test(m);
  const prefs = {
    gemini: [/^gemini-flash-latest$/, /^gemini-[\d.]+-flash$/, /flash/],
    openai: [/^gpt-[\d.]+-mini$/, /^gpt-4o-mini$/, /^gpt/],
    anthropic: [/haiku/, /sonnet/],
    openrouter: [/gpt-4o-mini/, /flash/],
    deepseek: [/^deepseek-chat$/],
    custom: [/./],
  }[pid] || [/./];
  const cands = list.filter(ok).sort().reverse();
  for (const re of prefs) { const m = cands.find((x) => re.test(x)); if (m) return m; }
  return cands[0] || null;
}
function calloutHtml(s) {
  return s.busy ? `<div class="spinner"></div><span>${esc(s.msg)}</span>` : `${icon(s.ok ? 'check' : 'info', 16, 2)}<span>${esc(s.msg)}</span>`;
}
function shortErr(e) {
  return String(e).replace(/\s*\(\{[\s\S]*$/, '').replace(/\s*\{[\s\S]*$/, '').slice(0, 220);
}

// ============================================================ HELP / FEEDBACK
function renderHelp() {
  const sc = S.shortcuts, w = esc(S.trigger_word);
  const card = (ico, title, body) => {
    const [t, keys] = title.split(' · ');
    return `<div class="help-card"><h3><span class="h-ico">${icon(ico, 15)}</span>${t}${keys ? `<span class="h-keys">${keys}</span>` : ''}</h3><div class="muted">${body}</div></div>`;
  };
  $('#view-help').innerHTML = `<h1>How to use Keeyz</h1><p class="lead">AI that types where you type. It works in any app: browsers, chats, email, documents, spreadsheets and code editors.</p>
    <div class="help-grid">${card('bolt', `Quick Actions · ${kbd(sc.actions)}`, 'Select text, press the shortcut, pick an action (Fix grammar, Shorten, Professional, Translate, Summarize, Reply…) or describe a change. Review the result, then Replace or Copy.')}
    ${card('keyboard', `Typed trigger · ${w}`, `Type your text, then <b>${w}</b> + an instruction, then press space twice. Example: <i>this meeting couldve been an email lol ${w} make it formal</i>. With nothing before it, it writes new text: <i>${w} write a thank-you note</i>. Reminders: <i>${w} remind me in 10 minutes to call mom</i>. Press Esc to cancel.`)}
    ${card('calc', 'Typed commands', '<code>/calc 12*8</code> · <code>/search best pizza</code> · <code>/yt lofi</code> · <code>/maps coffee near me</code>, then space twice. Add your own commands in Settings.')}
    ${card('wand', 'Selection toolbar', 'Select text with the mouse in any app: a small toolbar shows the buttons that fit. Your own text gets Fix, Professional, Shorten; a word gets Define; long text gets Summarize and Key points. Translate offers your recent languages, and ⋯ opens every action.')}
    ${card('reply', `Reply from screen · ${kbd(sc.reply || 'Ctrl+Shift+Y')}`, 'Click in the reply box of any chat or mail and press the shortcut. Keeyz reads the conversation on screen and writes the answer. Type a few words first to steer it.')}
    ${card('search', `Launcher · ${kbd(sc.launcher)}`, 'One search box for tools, actions and workflows. Type maths (12 * 8), search your clipboard, or press Tab to chat with the AI.')}
    ${card('mic', `Voice typing · ${kbd(sc.voice)}`, 'Dictate in 50+ languages. When you stop: Insert (cleaned up), Insert as heard, Continue, or send to Quick Actions.')}
    ${card('clipboard', `Clipboard history · ${kbd(sc.clipboard)}`, 'Search, pin, paste with 1–9, or run AI on any past clip. Images are kept too and can be read with OCR.')}
    ${card('scan', `Text from screen · ${kbd(sc.ocr)}`, 'Drag over text you can\'t select (subtitles, images, PDFs, error dialogs). Copy it or send it to Quick Actions. Uses the built-in Windows OCR, offline.')}
    ${card('layout', 'Snap windows · <kbd>Ctrl</kbd><kbd>Ctrl</kbd>', 'Double-tap Ctrl, then Ctrl + ← ↓ → for thirds, Ctrl + L / ; for halves, Ctrl + F for full screen. Triple-tap Ctrl to pick 2–9 columns. Esc ends it. More on the Snap page.')}
    ${card('undo', `Undo · ${kbd(sc.undo)}`, 'Undo the last AI change in your app. History keeps the last 20 changes.')}
    ${card('flows', 'Workflows', 'Chain steps so each feeds the next (Meeting notes → Action items → Follow-up email). Build your own in Flows.')}
    </div><div class="row" style="margin-top:16px"><button class="btn" id="showWelcome">Show welcome again</button></div>`;
  $('#showWelcome').onclick = () => welcome(true);
}

function renderFeedback() {
  $('#view-feedback').innerHTML = `<h1>Send feedback</h1><p class="lead">Tell us what works and what gets in your way. Notes are saved to <code>feedback.md</code> in the app data folder.</p>
    <textarea class="input" id="fb" style="min-height:160px" placeholder="What should Keeyz do better?"></textarea>
    <div class="row" style="margin-top:10px"><span class="spacer"></span><button class="btn primary" id="fbSend">Save feedback</button></div>`;
  $('#fbSend').onclick = async () => { const t = $('#fb').value.trim(); if (!t) return; const f = await invoke('save_feedback', { text: t }); $('#fb').value = ''; toast('Thanks! Saved', f); };
}

// ============================================================ MODAL / WELCOME
function openModal(html) { $('#modalCard').innerHTML = html; $('#modal').classList.remove('hidden'); }
function closeModal() { $('#modal').classList.add('hidden'); }
$('#modal').onclick = (e) => e.target.id === 'modal' && closeModal();

function welcome(force = false) {
  if (S.onboarded && !force) return;
  const sc = S.shortcuts, w = esc(S.trigger_word);
  openModal(`<div class="welcome-head"><img src="${logoUrl}" /><div><h1 style="margin:0;font-size:21px">Welcome to Keeyz</h1><div class="muted">AI that types where you type. Select text, press a key, done.</div></div></div>
    <div class="welcome-steps">
      <div class="welcome-step"><span class="n">1</span><div><b>Add your AI key</b><div class="muted small">OpenAI, Anthropic, Gemini, OpenRouter, DeepSeek or a local model. Stored only on this PC.</div></div></div>
      <div class="welcome-step"><span class="n">2</span><div><b>Select text, press ${kbd(sc.actions)}</b><div class="muted small">Pick Fix, Shorten, Translate… or describe the change.</div></div></div>
      <div class="welcome-step"><span class="n">3</span><div><b>Or type it: <i>your text ${w} make it shorter</i> + space twice</b><div class="muted small">Works in every app. Esc cancels.</div></div></div>
      <div class="welcome-step"><span class="n">4</span><div><b>Or just select text</b><div class="muted small">A small toolbar appears next to your selection with the right buttons for it. ⋯ opens every action.</div></div></div>
      <div class="welcome-step"><span class="n">5</span><div><b>Reply with one key: ${kbd(sc.reply || 'Ctrl+Shift+Y')}</b><div class="muted small">Click in the reply box of any chat or mail and press it. Keeyz reads the conversation on screen and writes the answer. Type a few words first to steer it.</div></div></div>
      <div class="welcome-step"><span class="n">6</span><div><b>Snap windows: double-tap <kbd>Ctrl</kbd></b><div class="muted small">Then Ctrl + ← ↓ → for thirds, Ctrl + L / ; for halves, Ctrl + F for full screen. Triple-tap for columns.</div></div></div>
      <div class="welcome-step"><span class="n">7</span><div><b>More tools</b><div class="muted small">Launcher ${kbd(sc.launcher)} · Voice ${kbd(sc.voice)} · Clipboard ${kbd(sc.clipboard)} · Text from screen ${kbd(sc.ocr)}</div></div></div>
    </div>
    <div class="row"><span class="faint small">Keeyz stays in your system tray so shortcuts work everywhere.</span><span class="spacer"></span>
      <button class="btn" id="wLater">Later</button><button class="btn grad" id="wKey">Add my AI key</button></div>`);
  const done = async () => { closeModal(); if (!S.onboarded) S = await saveSettings({ onboarded: true }); };
  $('#wLater').onclick = done;
  $('#wKey').onclick = async () => { await done(); show('provider'); };
}

show('home');
welcome();
