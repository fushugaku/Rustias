export function wavHeader(frames, sampleRate) {
  const bytes = frames * 4;
  if (!Number.isSafeInteger(frames) || frames < 0 || bytes > 0xffffffff - 36) throw new Error('This recording exceeds the WAV file size limit.');
  const header = new ArrayBuffer(44), view = new DataView(header);
  const text = (offset, value) => [...value].forEach((c, i) => view.setUint8(offset + i, c.charCodeAt(0)));
  text(0, 'RIFF'); view.setUint32(4, 36 + bytes, true); text(8, 'WAVE'); text(12, 'fmt ');
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 2, true);
  view.setUint32(24, sampleRate, true); view.setUint32(28, sampleRate * 4, true);
  view.setUint16(32, 4, true); view.setUint16(34, 16, true); text(36, 'data'); view.setUint32(40, bytes, true);
  return header;
}
export function recordingTime(seconds) {
  const n = Math.max(0, Math.floor(seconds));
  return n >= 3600 ? `${Math.floor(n / 3600)}:${String(Math.floor(n / 60) % 60).padStart(2, '0')}:${String(n % 60).padStart(2, '0')}` : `${Math.floor(n / 60)}:${String(n % 60).padStart(2, '0')}`;
}
export function recordingFilename(folder, name, format) {
  const safe = value => value.replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_').trim().slice(0, 96) || 'Recording';
  return `${safe(folder)} - ${safe(name)}.${format}`;
}
