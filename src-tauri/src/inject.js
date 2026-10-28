// Script inyectado en las ventanas de WhatsApp Web (initialization_script en
// create_whatsapp_window_impl, window.rs). Corre antes que cualquier JS de
// WhatsApp. Se embebe en el binario con include_str!; CI valida la sintaxis
// con `node --check`. Ver CLAUDE.md, sección "Script inyectado".
(function() {
    const _ipcOk = !!window.__TAURI_INTERNALS__;
    console.log('[whataJOST] init script cargado. IPC disponible:', _ipcOk);
    if (_ipcOk) {
        window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'Script de inicialización cargado (whataJOST)'
        });
    }

    // Mantener WhatsApp activo aunque la ventana esté oculta.
    // Sin esto, document.visibilityState pasa a "hidden" y WhatsApp
    // pausa la recepción de mensajes y actualizaciones.
    try {
        Object.defineProperty(document, 'visibilityState', { get: () => 'visible', configurable: true });
        Object.defineProperty(document, 'hidden', { get: () => false, configurable: true });
    } catch(e) {}

    const isWhatsApp = (url) =>
        /whatsapp\.(com|net)|facebook\.com|fbcdn\.net/.test(new URL(url).hostname);

    // Rastrear el último elemento editable enfocado.
    // Al hacer drag & drop, document.activeElement puede ser body si el foco
    // se perdió durante el arrastre; con este rastreo podemos re-enfocar el
    // input del chat antes de despachar el paste sintético.
    document.addEventListener('focus', function(e) {
        var el = e.target;
        if (el && (el.isContentEditable || el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) {
            window.__waLastFocusedEditable = el;
        }
    }, true);

    window.dispatchPasteWithFiles = function(dt) {
        if (!dt.files.length) {
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', { level: 'warn', message: 'dispatchPasteWithFiles: DataTransfer vacío, no se despacha' });
            return;
        }
        var stored = window.__waLastFocusedEditable;
        var target = (stored && document.contains(stored))
            ? stored
            : (document.querySelector('div[contenteditable][data-tab]') ||
               document.querySelector('div[contenteditable="true"]') ||
               document.activeElement ||
               document.body);
        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'dispatchPasteWithFiles: ' + dt.files.length + ' archivo(s), target=' + (target.tagName || 'desconocido') + ' editable=' + (target.isContentEditable || false)
        });
        if (target !== document.body && target.focus) try { target.focus(); } catch(_) {}
        var pe;
        try { pe = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt }); }
        catch(_) {
            pe = new Event('paste', { bubbles: true, cancelable: true });
            Object.defineProperty(pe, 'clipboardData', { get: function() { return dt; }, configurable: true });
        }
        target.dispatchEvent(pe);
    };

    window.open = function(url) {
        if (url) try {
            if (url.startsWith('blob:')) {
                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'window.open(blob:) interceptado, descargando'
                });
                downloadBlob(url, 'archivo');
                return null;
            }
            if (!isWhatsApp(url)) {
                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'Link externo interceptado (window.open): ' + url
                });
                window.location.href = url;
                return null;
            }
        } catch(e) {
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'error',
                message: 'Error en window.open: ' + (e.message || e)
            });
        }
        return null;
    };

    // Track blob URLs to avoid fetch race conditions
    const blobStore = new Map();
    const origCreateObjectURL = URL.createObjectURL;
    URL.createObjectURL = function(blob) {
        const url = origCreateObjectURL.call(URL, blob);
        blobStore.set(url, blob);
        return url;
    };
    const origRevokeObjectURL = URL.revokeObjectURL;
    URL.revokeObjectURL = function(url) {
        setTimeout(() => blobStore.delete(url), 60000);
        origRevokeObjectURL.call(URL, url);
    };

    // Helper: send the blob to Rust in chunks and invoke save_file.
    // WhatsApp's CSP blocks Tauri's binary ipc:// protocol, so IPC goes
    // through postMessage as JSON; base64 chunks keep peak memory at
    // ~one chunk instead of several full-size copies of the file.
    const SAVE_CHUNK_SIZE = 4 * 1024 * 1024;
    const blobToBase64 = (blob) => new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result.split(',')[1] || '');
        reader.onerror = reject;
        reader.readAsDataURL(blob);
    });
    const saveBlob = async (blob, fileName) => {
        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'saveBlob: enviando blob (' + blob.size + ' bytes) para ' + fileName
        });
        const id = await window.__TAURI_INTERNALS__.invoke('save_file_begin');
        try {
            for (let offset = 0; offset < blob.size; offset += SAVE_CHUNK_SIZE) {
                const data = await blobToBase64(blob.slice(offset, offset + SAVE_CHUNK_SIZE));
                await window.__TAURI_INTERNALS__.invoke('save_file_chunk', { id, data });
            }
        } catch (e) {
            window.__TAURI_INTERNALS__.invoke('save_file_abort', { id }).catch(() => {});
            throw e;
        }
        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'saveBlob: invocando save_file para ' + fileName
        });
        await window.__TAURI_INTERNALS__.invoke('save_file', {
            id,
            fileName: fileName || 'archivo'
        });
    };

    const downloadBlob = (url, fileName) => {
        console.log('[whataJOST] downloadBlob iniciado:', fileName || '(sin nombre)', '| IPC:', !!window.__TAURI_INTERNALS__);
        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'downloadBlob iniciado: ' + (fileName || '(sin nombre)')
        });
        (async () => {
            try {
                let blob = blobStore.get(url);
                if (!blob) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'info',
                        message: 'Blob no en caché, haciendo fetch de URL'
                    });
                    const response = await fetch(url);
                    if (!response.ok) throw new Error('fetch failed: ' + response.status);
                    blob = await response.blob();
                }
                await saveBlob(blob, fileName);
            } catch(e) {
                try {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'error',
                        message: 'Error al descargar archivo ' + fileName + ': ' + (e.message || e)
                    });
                } catch (_) {
                    console.error('[whataJOST] Error al descargar (IPC no disponible):', e);
                }
            }
        })();
    };

    // Exponer downloadBlob globalmente para que Rust pueda llamarla via eval
    window.__waDownloadBlob = (url, fileName) => downloadBlob(url, fileName || 'archivo');

    // Catch programmatic anchor clicks (WhatsApp Web creates <a> elements and calls .click())
    const origAnchorClick = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function() {
        // Interceptar cualquier blob: URL (con o sin atributo download)
        if (this.href && this.href.startsWith('blob:')) {
            console.log('[whataJOST] prototype.click blob interceptado:', this.download || '(sin attr)');
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'info',
                message: 'Descarga interceptada (prototype.click): ' + (this.download || '(sin attr)')
            });
            downloadBlob(this.href, this.download || 'archivo');
            return;
        }
        return origAnchorClick.call(this);
    };

    // Catch user clicks on <a> elements (for anchors already in the DOM)
    document.addEventListener('click', function(e) {
        for (const node of e.composedPath()) {
            if (node.tagName === 'A' && node.href) {
                try {
                    // Interceptar cualquier blob: URL (con o sin atributo download)
                    if (node.href.startsWith('blob:')) {
                        e.preventDefault();
                        e.stopImmediatePropagation();
                        console.log('[whataJOST] click event blob interceptado:', node.download || '(sin attr)');
                        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                            level: 'info',
                            message: 'Descarga interceptada (click event): ' + (node.download || '(sin attr)')
                        });
                        downloadBlob(node.href, node.download || 'archivo');
                        break;
                    }
                    if (!isWhatsApp(node.href)) {
                        e.preventDefault();
                        e.stopImmediatePropagation();
                        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                            level: 'info',
                            message: 'Link externo interceptado (click): ' + node.href
                        });
                        window.location.href = node.href;
                    }
                } catch(e) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'error',
                        message: 'Error en click handler: ' + (e.message || e)
                    });
                }
                break;
            }
        }
    }, true);

    // Allow native context menu on editable fields for spellcheck suggestions.
    // WhatsApp Web intercepts right-clicks via contextmenu/mousedown/mouseup
    // to show its formatting menu. We stop those events in capture phase so
    // the native WebKitGTK menu (with spellcheck) can appear instead.
    const isEditable = (el) => {
        if (!el) return false;
        return el.isContentEditable
            || el.tagName === 'INPUT'
            || el.tagName === 'TEXTAREA';
    };
    const resolveTarget = (e) =>
        e.target && e.target.nodeType === Node.TEXT_NODE
            ? e.target.parentElement
            : e.target;

    document.addEventListener('contextmenu', function(e) {
        if (isEditable(resolveTarget(e))) {
            e.stopImmediatePropagation();
        }
    }, true);
    document.addEventListener('mousedown', function(e) {
        if (e.button === 2 && isEditable(resolveTarget(e))) {
            e.stopImmediatePropagation();
        }
    }, true);
    document.addEventListener('mouseup', function(e) {
        if (e.button === 2 && isEditable(resolveTarget(e))) {
            e.stopImmediatePropagation();
        }
    }, true);

    // Interceptar evento paste para pegar imágenes desde el portapapeles del sistema
    document.addEventListener('paste', async function(e) {
        // Si el evento ya trae archivos, no interferir
        if (e.clipboardData && e.clipboardData.files && e.clipboardData.files.length > 0) {
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'info',
                message: 'Paste con archivos del sistema (' + e.clipboardData.files.length + '), dejando pasar a WhatsApp'
            });
            return;
        }

        // Si WebKit ya expone la imagen en clipboardData.items, usarla directo
        const clipItems = e.clipboardData ? Array.from(e.clipboardData.items || []) : [];
        const imageItem = clipItems.find(function(it) { return it.type && it.type.startsWith('image/'); });
        if (imageItem) {
            e.preventDefault();
            e.stopImmediatePropagation();
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'info',
                message: 'Imagen en clipboardData.items (' + imageItem.type + '), procesando directo'
            });
            var imgFile = imageItem.getAsFile();
            if (imgFile) {
                var imgDt = new DataTransfer();
                imgDt.items.add(imgFile);
                var imgPasteEvent;
                try {
                    imgPasteEvent = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: imgDt });
                } catch(_) {
                    imgPasteEvent = new Event('paste', { bubbles: true, cancelable: true });
                    Object.defineProperty(imgPasteEvent, 'clipboardData', { get: function() { return imgDt; }, configurable: true });
                }
                (document.activeElement || document.body).dispatchEvent(imgPasteEvent);
            }
            return;
        }

        const clipTypes = e.clipboardData ? Array.from(e.clipboardData.types || []) : [];

        // Solo text/plain o text/rtf indican un paste de texto real → dejar pasar a WhatsApp.
        // text/html sin text/plain puede ser una imagen envuelta en HTML
        // (ej: Firefox copia imagen → clipboard tiene image/png + text/html con <img>).
        // WebKit2GTK no expone el image/png externo en clipboardData.items, pero wl-paste sí
        // puede leerlo del clipboard del sistema → dejamos que caiga al path IPC.
        if (clipTypes.some(function(t) { return t === 'text/plain' || t === 'text/rtf'; })) {
            return;
        }
        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'Paste sin texto plano. Tipos: ' + (clipTypes.join(', ') || '(vacío)') + '. Intentando IPC...'
        });

        // IMPORTANTE: detener el evento AHORA (sincrónico) para que WhatsApp no
        // procese el paste mientras esperamos la respuesta IPC (async).
        e.preventDefault();
        e.stopImmediatePropagation();

        // Obtener imagen del portapapeles via IPC
        let clipImage = null;
        if (window.__TAURI_INTERNALS__) {
            try {
                clipImage = await window.__TAURI_INTERNALS__.invoke('read_clipboard_image');
            } catch(err) {
                window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'error',
                    message: 'Error al invocar read_clipboard_image: ' + (err.message || err)
                });
            }
        }
        if (!clipImage && window.__tauriClipboardImage) {
            clipImage = { data: window.__tauriClipboardImage, mime_type: 'image/png' };
        }
        if (!clipImage) {
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'warn',
                message: 'No hay imagen en el portapapeles (tipos JS: ' + (clipTypes.join(', ') || '(vacío)') + ')'
            });
            return;
        }

        // Convertir base64 a File
        const b64 = clipImage.data;
        const mimeType = clipImage.mime_type || 'image/png';
        const binary = atob(b64);
        const array = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) array[i] = binary.charCodeAt(i);
        const blob = new Blob([array], { type: mimeType });
        const ext = mimeType.split('/')[1] || 'png';
        const file = new File([blob], 'clipboard.' + ext, { type: mimeType });

        // Crear DataTransfer con el archivo y despachar un paste sintético.
        // Así WhatsApp Web lo procesa como imagen normal (no como sticker,
        // que es lo que ocurre al inyectar vía <input type="file"> + change).
        const dt = new DataTransfer();
        dt.items.add(file);

        let pasteEvent;
        try {
            pasteEvent = new ClipboardEvent('paste', {
                bubbles: true,
                cancelable: true,
                clipboardData: dt
            });
        } catch(_) {
            pasteEvent = new Event('paste', { bubbles: true, cancelable: true });
            Object.defineProperty(pasteEvent, 'clipboardData', {
                get: function() { return dt; },
                configurable: true
            });
        }

        const target = document.activeElement || document.body;
        target.dispatchEvent(pasteEvent);
    }, true);

    // Interceptar drag & drop para pegar archivos en la conversación
    var _dragLogged = false;
    document.addEventListener('dragover', function(e) {
        e.preventDefault();
        e.stopPropagation();
        if (!_dragLogged) {
            _dragLogged = true;
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'info',
                message: 'dragover DOM alcanzado (primera vez)'
            });
        }
    }, true);

    document.addEventListener('drop', function(e) {
        e.preventDefault();
        e.stopPropagation();
        _dragLogged = false;

        var dt = e.dataTransfer;
        var filesLen = dt ? dt.files.length : -1;
        var itemsLen = dt ? dt.items.length : -1;
        var itemsDesc = '';
        if (dt && dt.items) {
            for (var ii = 0; ii < dt.items.length; ii++) {
                itemsDesc += '[' + dt.items[ii].kind + '/' + dt.items[ii].type + ']';
            }
        }
        var types = dt ? Array.from(dt.types || []).join(',') : '';
        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'DOM drop: files=' + filesLen + ' items=' + itemsLen + ' itemsDesc=' + itemsDesc + ' types=[' + types + ']'
        });

        if (!dt) return;

        // Path 1: archivos directamente en dataTransfer.files
        if (filesLen > 0) {
            const newDt = new DataTransfer();
            for (let i = 0; i < filesLen; i++) newDt.items.add(dt.files[i]);
            dispatchPasteWithFiles(newDt);
            return;
        }

        // Path 1b: items con kind=file (algunos browsers exponen items pero no files)
        if (itemsLen > 0) {
            const fileItems = [];
            for (let i = 0; i < itemsLen; i++) {
                if (dt.items[i].kind === 'file') fileItems.push(dt.items[i].getAsFile());
            }
            if (fileItems.length > 0 && fileItems[0]) {
                const newDt = new DataTransfer();
                fileItems.forEach(function(f) { if (f) newDt.items.add(f); });
                dispatchPasteWithFiles(newDt);
                return;
            }
        }

        // Path 2: En Linux/WebKit2GTK los archivos no están en dataTransfer.files.
        // text/uri-list contiene las URIs de los archivos (file://) o URLs (https://).
        const rawUris = e.dataTransfer.getData('text/uri-list');
        if (!rawUris) return;
        const uris = rawUris.split(/\r?\n/).map(function(u) { return u.trim(); }).filter(function(u) { return u && !u.startsWith('#'); });
        if (!uris.length) return;

        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
            level: 'info',
            message: 'Drop via text/uri-list: ' + uris.join(', ')
        });

        (async function() {
            const dt = new DataTransfer();
            for (const uri of uris) {
                try {
                    if (uri.startsWith('file://')) {
                        if (!window.__TAURI_INTERNALS__) continue;
                        const path = decodeURIComponent(new URL(uri).pathname);
                        const res = await window.__TAURI_INTERNALS__.invoke('read_file_for_drop', { path });
                        if (res) {
                            const bin = atob(res.data), arr = new Uint8Array(bin.length);
                            for (let i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
                            dt.items.add(new File([new Blob([arr], { type: res.mime })], res.name, { type: res.mime }));
                        }
                    } else if (uri.startsWith('http://') || uri.startsWith('https://')) {
                        const resp = await fetch(uri);
                        if (!resp.ok) throw new Error('HTTP ' + resp.status);
                        const blob = await resp.blob();
                        const name = decodeURIComponent(uri.split('/').pop().split('?')[0]) || 'archivo';
                        dt.items.add(new File([blob], name, { type: blob.type || 'application/octet-stream' }));
                    }
                } catch(err) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'error',
                        message: 'Drop: error con URI ' + uri + ': ' + (err.message || err)
                    });
                }
            }
            dispatchPasteWithFiles(dt);
        })();
    }, true);

    // Intercept Notification API to show in-app toast
    const _OrigNotification = window.Notification;
    window.Notification = function(title, options) {
        try {
            window.__TAURI_INTERNALS__.invoke('show_notification', {
                title: title || '',
                body: (options && options.body) ? options.body : ''
            });
        } catch(e) {
            console.warn('[whataJOST] notification invoke failed:', e);
        }
    };
    window.Notification.permission = 'granted';
    window.Notification.requestPermission = function() {
        return Promise.resolve('granted');
    };
    Object.defineProperty(window.Notification, 'permission', {
        get: () => 'granted',
        configurable: true
    });


    // Watch document.title for unread message count
    (function() {
        let lastCount = -1;
        let titleObs = null;
        let titleObsTarget = null;
        function checkCount() {
            const title = document.title;
            const m = title.match(/^\((\d+)\)/);
            const count = m ? (parseInt(m[1], 10) || 0) : 0;
            if (count !== lastCount) {
                lastCount = count;
                try { window.__TAURI_INTERNALS__.invoke('update_unread_count', { count: count }); } catch(e) {}
            }
        }
        function attachTitleObserver() {
            const titleEl = document.querySelector('title');
            if (titleEl && !titleObs) {
                titleObs = new MutationObserver(checkCount);
                titleObsTarget = titleEl;
                titleObs.observe(titleEl, { childList: true, characterData: true, subtree: true });
                checkCount();
            }
        }
        // Observe the document until WhatsApp inserts its <title>, then
        // disconnect: a subtree observer on the whole DOM would otherwise
        // fire on every WhatsApp mutation for the whole session.
        attachTitleObserver();
        if (!titleObs) {
            const headObs = new MutationObserver(function() {
                attachTitleObserver();
                if (titleObs) headObs.disconnect();
            });
            headObs.observe(document.documentElement, { childList: true, subtree: true });
        }
        // Safety net in case WhatsApp replaces the <title> element itself
        // (the observer would stay attached to the detached node).
        setInterval(function() {
            const titleEl = document.querySelector('title');
            if (titleObs && titleEl && titleEl !== titleObsTarget) {
                titleObs.disconnect();
                titleObs = null;
                attachTitleObserver();
            }
            checkCount();
        }, 30000);
    })();

    // --- Drag & drop: prevent WebView from intercepting file drops as navigation ---
    document.addEventListener('dragover', function(e) { e.preventDefault(); }, false);

    // Bridge: Rust calls this after reading a dropped file (fallback when
    // WebKitGTK intercepts the native drop and tries to navigate to a file:// URL).
    // We dispatch a paste event (not a drop event) because WhatsApp Web accepts
    // synthetic ClipboardEvents but ignores synthetic DragEvents (isTrusted check).
    window.__tauriInjectDrop = function(fileName, base64Data, mimeType) {
        try {
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'info',
                message: '__tauriInjectDrop: ' + fileName + ' (' + (mimeType || 'octet-stream') + ')'
            });
            const binary = atob(base64Data);
            const array = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) array[i] = binary.charCodeAt(i);
            const blob = new Blob([array], { type: mimeType || 'application/octet-stream' });
            const file = new File([blob], fileName, { type: mimeType || 'application/octet-stream' });
            const dt = new DataTransfer();
            dt.items.add(file);
            dispatchPasteWithFiles(dt);
        } catch(e) {
            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                level: 'error',
                message: '__tauriInjectDrop error: ' + (e.message || e)
            });
        }
    };
})();
