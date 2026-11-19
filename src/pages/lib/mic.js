// Small "speak your instruction" helper: records until the user stops talking, then transcribes.
// Used by the mic buttons in Quick Actions.
import { invoke, toast } from './core.js';

let active = null;

/** Toggle recording for `btn`. Calls `onText(text)` with the transcript. */
export async function micToggle(btn, onText, settings = {}) {
  if (active) return active.stop();
  let stream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: settings.voice_mic ? { deviceId: { exact: settings.voice_mic } } : true });
  } catch {
    return toast("The microphone can't be opened", 'Allow microphone access in Windows Settings → Privacy → Microphone.', true);
  }
  const mime = MediaRecorder.isTypeSupported('audio/webm;codecs=opus') ? 'audio/webm;codecs=opus' : 'audio/webm';
  const rec = new MediaRecorder(stream, { mimeType: mime });
  const chunks = [];
  rec.ondataavailable = (e) => e.data.size && chunks.push(e.data);
  rec.start(250);
  const actx = new AudioContext();
  const an = actx.createAnalyser();
  an.fftSize = 512;
  actx.createMediaStreamSource(stream).connect(an);
  const data = new Uint8Array(an.frequencyBinCount);
  let heard = false, silentSince = 0, raf = 0, stopped = false;
  const t0 = Date.now();
  btn.classList.add('mic-live');
  const placeholderEl = btn.closest('.prompt, .refine')?.querySelector('input');
  const oldPh = placeholderEl?.placeholder;
  if (placeholderEl) placeholderEl.placeholder = 'Listening… say what you want';

  const stop = async () => {
    if (stopped) return;
    stopped = true;
    active = null;
    cancelAnimationFrame(raf);
    if (rec.state !== 'inactive') await new Promise((r) => { rec.onstop = r; rec.stop(); });
    stream.getTracks().forEach((t) => t.stop());
    try { await actx.close(); } catch {}
    btn.classList.remove('mic-live');
    const blob = new Blob(chunks, { type: rec.mimeType });
    if (!heard || blob.size < 2000) {
      if (placeholderEl) placeholderEl.placeholder = oldPh;
      return toast("Didn't catch that", 'Nothing was heard. Check the microphone and try again.');
    }
    if (placeholderEl) placeholderEl.placeholder = 'Transcribing…';
    try {
      const text = await invoke('voice_transcribe', { audio: Array.from(new Uint8Array(await blob.arrayBuffer())), mime: rec.mimeType });
      if (text?.trim()) onText(text.trim());
    } catch (e) {
      toast('Could not transcribe', String(e), true);
    }
    if (placeholderEl) placeholderEl.placeholder = oldPh;
  };
  active = { stop };

  const tick = () => {
    an.getByteTimeDomainData(data);
    let sum = 0;
    for (const v of data) sum += ((v - 128) / 128) ** 2;
    const rms = Math.sqrt(sum / data.length);
    if (rms > 0.03) { heard = true; silentSince = 0; }
    else if (heard && !silentSince) silentSince = Date.now();
    // stop by itself after a short pause, or after 60 s
    if ((heard && silentSince && Date.now() - silentSince > 1500) || Date.now() - t0 > 60_000) return stop();
    raf = requestAnimationFrame(tick);
  };
  tick();
}
