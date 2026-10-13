(() => {
  const native = window.RustiasFiles;
  if (!native) return;
  const waiting = new Map();
  let serial = 0, exporting = false;
  native.onmessage = event => {
    const data = JSON.parse(event.data), pending = waiting.get(data.request);
    if (!pending) return;
    waiting.delete(data.request); clearTimeout(pending.timer);
    if (data.ok) pending.resolve(data);
    else pending.reject(new Error(data.error || 'Could not export the file.'));
  };
  function send(message, timeout = 30000) {
    return new Promise((resolve, reject) => {
      const request = ++serial;
      const timer = timeout ? setTimeout(() => { waiting.delete(request); reject(new Error('The Android file export did not respond.')); }, timeout) : null;
      waiting.set(request, {resolve, reject, timer});
      native.postMessage(JSON.stringify({...message, request}));
    });
  }
  function report(error) {
    const alert = document.querySelector('#error');
    if (alert) { alert.textContent = error.message; alert.hidden = false; }
  }
  async function exportBlob(blob, name) {
    if (exporting) throw new Error('Finish the current file export first.');
    exporting = true;
    try {
      await send({type:'begin', name, mime:blob.type || 'application/octet-stream', size:blob.size});
      for (let offset = 0; offset < blob.size; offset += 128 * 1024) {
        const bytes = new Uint8Array(await blob.slice(offset, offset + 128 * 1024).arrayBuffer());
        let binary = '';
        for (let start = 0; start < bytes.length; start += 8192) binary += String.fromCharCode(...bytes.subarray(start, start + 8192));
        const result = await send({type:'chunk', offset, data:btoa(binary)});
        if (result.offset !== offset + bytes.length) throw new Error('The Android export has an incomplete chunk.');
      }
      // The user may keep the system picker open as long as needed.
      return await send({type:'finish'}, 0);
    } catch (error) {
      await send({type:'abort'}, 5000).catch(() => {});
      throw error;
    } finally { exporting = false; }
  }
  function setFullscreen(active) {
    document.body.dataset.fullscreen = active ? 'on' : 'off';
    const button = document.querySelector('#fullscreen');
    button?.setAttribute('aria-pressed', String(active));
    button?.setAttribute('aria-label', active ? 'Exit full screen' : 'Full screen');
    if (button) button.title = active ? 'Exit full screen' : 'Full screen';
  }
  window.RustiasAndroid = {
    exportBlob, setFullscreen,
    pause() {
      window.dispatchEvent(new Event('pagehide'));
      document.querySelector('#panic')?.click();
      document.querySelectorAll('audio').forEach(audio => audio.pause());
      const power = document.querySelector('#power');
      if (power?.textContent.includes('Pause')) power.click();
    },
    back() {
      const dialogs = [...document.querySelectorAll('dialog[open]')];
      if (dialogs.length) { dialogs.at(-1).close(); return true; }
      const popovers = [...document.querySelectorAll(':popover-open')];
      if (popovers.length) { popovers.at(-1).hidePopover(); return true; }
      return false;
    },
  };
  // Run after the existing target handlers have prepared the JSON/blob URL.
  document.addEventListener('click', event => {
    const link = event.target.closest?.('a[download]');
    if (event.defaultPrevented || !link?.href.startsWith('blob:')) return;
    event.preventDefault(); link.setAttribute('aria-busy', 'true');
    fetch(link.href).then(response => response.blob()).then(blob => exportBlob(blob, link.download))
      .catch(report).finally(() => link.removeAttribute('aria-busy'));
  });
  document.addEventListener('click', event => {
    if (!event.target.closest?.('#fullscreen')) return;
    event.preventDefault(); event.stopImmediatePropagation();
    send({type:'fullscreen', active:document.body.dataset.fullscreen !== 'on'})
      .then(result => setFullscreen(result.fullscreen)).catch(report);
  }, true);
  document.body.dataset.platform = 'android';
})();
