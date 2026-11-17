import { boot, invoke, listen, win, $, $$, esc, toast, hide, saveSettings, sound } from './lib/core.js';
import { VOICE_LANGS } from './lib/langs.js';
import { icon, actIcon } from './lib/icons.js';
document.querySelectorAll('[data-icon]').forEach((b) => b.insertAdjacentHTML('afterbegin', icon(b.dataset.icon, b.id === 'mic' ? 20 : 15)));


let S = await boot();
document.addEventListener('settings', (e) => (S = e.detail));

const MAX_SECONDS = 300;
let rec, stream, chunks = [], actx, raf, t0, silentSince = 0, heard = false, finishing = false, mode = 'idle';
let prefix = ''; // text kept when the user presses Continue

$('#lang').innerHTML = VOICE_LANGS.map(([c, n]) => `<option value="${c}">${n}</option>`).join('');
$('#lang').value = S.voice_lang || 'auto';
$('#lang').onchange = async (e) => (S = await saveSettings({ voice_lang: e.target.value }));
$('#bars').innerHTML = '<i></i>'.repeat(28);

listen('voice-start', () => start(''));
listen('voice-toggle', () => (mode === 'recording' ? finish('auto') : mode === 'review' ? $('#ins').click() : start('')));
start('');

$('#done').onclick = () => finish('review');
$('#close').onclick = cancel;
$('#discard').onclick = () => { if (!$('#text').value.trim() || confirm('Discard this dictation?')) cancel(); };
$('#cont').onclick = () => start($('#text').value.trim());
$('#ins').onclick = () => insert(true);
$('#raw').onclick = () => insert(false);
$('#qa').onclick = async () => { const text = $('#text').value; await stopAll(); await hide(); invoke('open_actions_with', { text, source: 'voice', action: null }); };

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') { e.preventDefault(); cancel(); }
  if (mode === 'recording' && e.key === 'Enter') { e.preventDefault(); finish('auto'); }
  if (mode === 'recording' && e.key === 'Tab') { e.preventDefault(); finish('raw'); }
  if (mode === 'review' && e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); insert(true); }
  if (mode === 'review' && e.key === 'Tab') { e.preventDefault(); insert(false); }
});

function view(m) {
  mode = m;
  $('#recView').classList.toggle('hidden', m === 'review');
  $('#review').classList.toggle('hidden', m !== 'review');
  $('#foot').classList.toggle('hidden', m === 'review');
}

async function start(keep) {
  if (mode === 'recording') return;
  prefix = keep;
  finishing = false; heard = false; silentSince = 0; chunks = [];
  view('recording');
  $('#state').textContent = 'Listening…';
  $('#hint').textContent = S.voice_auto_stop ? 'Stops by itself when you stop talking' : 'Press the voice shortcut again to finish';
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: S.voice_mic ? { deviceId: { exact: S.voice_mic } } : true });
  } catch (e) {
    view('review');
    $('#state').textContent = 'Microphone blocked';
    $('#text').value = prefix;
    toast("The microphone can't be opened", 'Allow microphone access in Windows Settings → Privacy → Microphone, or pick another microphone in Settings.', true);
    return;
  }
  const mime = MediaRecorder.isTypeSupported('audio/webm;codecs=opus') ? 'audio/webm;codecs=opus' : 'audio/webm';
  rec = new MediaRecorder(stream, { mimeType: mime });
  rec.ondataavailable = (e) => e.data.size && chunks.push(e.data);
  rec.start(250);
  sound('start');
  actx = new AudioContext();
  const an = actx.createAnalyser();
  an.fftSize = 512;
  actx.createMediaStreamSource(stream).connect(an);
  const data = new Uint8Array(an.frequencyBinCount);
  const bars = $$('#bars i');
  t0 = Date.now();
  const tick = () => {
    an.getByteTimeDomainData(data);
    let sum = 0;
    for (const v of data) sum += ((v - 128) / 128) ** 2;
    const rms = Math.sqrt(sum / data.length);
    bars.forEach((b, i) => (b.style.height = Math.max(4, Math.min(34, rms * 260 * (0.5 + Math.abs(Math.sin(Date.now() / 140 + i))))) + 'px'));
    const secs = (Date.now() - t0) / 1000;
    $('#timer').textContent = `${Math.floor(secs / 60)}:${String(Math.floor(secs % 60)).padStart(2, '0')}`;
    if (rms > 0.03) { heard = true; silentSince = 0; $('#state').textContent = 'Hearing you…'; }
    else if (heard && !silentSince) silentSince = Date.now();
    if (S.voice_auto_stop && heard && silentSince && Date.now() - silentSince > 1800) return finish('auto');
    if (secs > MAX_SECONDS) { toast('Max duration reached'); return finish('review'); }
    raf = requestAnimationFrame(tick);
  };
  tick();
}

async function stopAll() {
  cancelAnimationFrame(raf);
  if (rec && rec.state !== 'inactive') {
    await new Promise((r) => { rec.onstop = r; rec.stop(); });
  }
  stream?.getTracks().forEach((t) => t.stop());
  try { await actx?.close(); } catch {}
}

/** how: 'auto' = paste right away unless review is on, 'review' = show text, 'raw' = paste as heard */
async function finish(how) {
  if (finishing || mode !== 'recording') return;
  finishing = true;
  await stopAll();
  $('#state').textContent = 'Transcribing…';
  $('#mic').classList.remove('live');
  const blob = new Blob(chunks, { type: rec.mimeType });
  if (!heard || blob.size < 2000) {
    view('review');
    $('#text').value = prefix;
    $('#state').textContent = "Didn't catch that";
    toast("Didn't catch that", 'Nothing was heard. Check the microphone and try again.');
    return;
  }
  let text = '';
  try {
    text = await invoke('voice_transcribe', { audio: Array.from(new Uint8Array(await blob.arrayBuffer())), mime: rec.mimeType });
  } catch (e) {
    view('review');
    $('#text').value = prefix;
    $('#state').textContent = 'Could not transcribe';
    return toast('Could not transcribe', String(e), true);
  }
  const full = [prefix, text].filter(Boolean).join(' ');
  $('#text').value = full;
  $('#mic').classList.add('live');
  if (how === 'raw') return insert(false);
  if (how === 'auto' && !S.voice_review) return insert(true);
  view('review');
  $('#state').textContent = 'Got it';
  $('#text').focus();
}

async function insert(polish) {
  let text = $('#text').value.trim();
  if (!text) return cancel();
  $('#state').textContent = polish ? 'Writing…' : 'Inserting…';
  if (polish) {
    try { text = await invoke('voice_polish', { text }); } catch (e) { toast('Polish failed, pasting as heard', String(e)); }
  }
  mode = 'idle';
  await invoke('paste_into_last', { text });
  sound('done');
}

async function cancel() {
  await stopAll();
  mode = 'idle';
  hide();
}
