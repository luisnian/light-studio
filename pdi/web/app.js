// RawStudioPDI · PWA móvil. Local primero (IndexedDB); sincroniza con el Worker cuando hay cobertura.
const API = '/api/v1';
const $ = (id) => document.getElementById(id);
const uuid = () => crypto.randomUUID();
const nowIso = () => new Date().toISOString();

// ---------- IndexedDB ----------
const idb = new Promise((res, rej) => {
  const r = indexedDB.open('rawstudiopdi', 1);
  r.onupgradeneeded = () => {
    const d = r.result;
    d.createObjectStore('pois', { keyPath: 'id' });
    d.createObjectStore('photos', { keyPath: 'att' }); // fotos propias pendientes o ya subidas
    d.createObjectStore('meta', { keyPath: 'k' });
  };
  r.onsuccess = () => res(r.result);
  r.onerror = () => rej(r.error);
});
async function store(name, mode = 'readonly') { return (await idb).transaction(name, mode).objectStore(name); }
const wrap = (req) => new Promise((res, rej) => { req.onsuccess = () => res(req.result); req.onerror = () => rej(req.error); });
const dbGet = async (s, k) => wrap((await store(s)).get(k));
const dbAll = async (s) => wrap((await store(s)).getAll());
const dbPut = async (s, v) => wrap((await store(s, 'readwrite')).put(v));
const dbDel = async (s, k) => wrap((await store(s, 'readwrite')).delete(k));
const metaGet = async (k, d = null) => (await dbGet('meta', k))?.v ?? d;
const metaSet = (k, v) => dbPut('meta', { k, v });

// ---------- Estado ----------
let pois = [];          // PDI locales (incluye los marcados para borrar; se filtran al pintar)
let pos = null;         // última posición GPS {lat, lon, acc}
let map, markers, meMarker;
let editing = null;     // PDI abierto en la ficha
let pendingPos = null;  // posición ajustada a mano en la ficha (se aplica al guardar)
let photoPos = null;    // ubicación EXIF de una foto añadida en la ficha
let pendingPhotos = []; // fotos nuevas de la ficha abierta
let syncing = false;

const live = () => pois.filter((p) => !p._del && !p.deleted_at).sort((a, b) => (b.created_at || '').localeCompare(a.created_at || ''));
const reload = async () => { pois = await dbAll('pois'); render(); };

function toast(msg, ms = 2600) {
  const t = $('toast'); t.textContent = msg; t.hidden = false;
  clearTimeout(toast.t); toast.t = setTimeout(() => (t.hidden = true), ms);
}

// ---------- Contrato ----------
function contract(p) {
  return {
    schema_version: 1, id: p.id, name: p.name, description: p.description || '', latitude: p.latitude, longitude: p.longitude,
    status: p.status, best_time: p.best_time || '', notes: p.notes || '', category: p.category || 'otro', tags: p.tags || [],
    created_at: p.created_at, origin: p.origin || 'mobile', attachments: p.attachments || [], accuracy_m: p.accuracy_m ?? null,
  };
}
async function save(p) { // guarda en local y deja el PDI pendiente de enviar
  p._dirty = true; p._op = uuid(); p._local_at = nowIso();
  await dbPut('pois', p);
  const i = pois.findIndex((x) => x.id === p.id);
  if (i >= 0) pois[i] = p; else pois.push(p);
  render();
  sync(true);
}

// ---------- GPS ----------
const recent = []; // lecturas recientes del GPS, para promediarlas al guardar
function startGps() {
  if (!('geolocation' in navigator)) { $('gps').textContent = 'Sin GPS en este navegador'; return; }
  navigator.geolocation.watchPosition(
    (g) => {
      pos = { lat: g.coords.latitude, lon: g.coords.longitude, acc: g.coords.accuracy };
      recent.push({ ...pos, t: Date.now() }); while (recent.length > 30) recent.shift();
      updateAdjustGps();
      $('gps').textContent = `GPS ±${Math.round(pos.acc)} m · ${pos.lat.toFixed(5)}, ${pos.lon.toFixed(5)}`;
      if (map) {
        if (!meMarker) { meMarker = L.circleMarker([pos.lat, pos.lon], { radius: 8, color: '#fff', weight: 2, fillColor: '#0a84ff', fillOpacity: 1 }).addTo(map); map.setView([pos.lat, pos.lon], 15); }
        else meMarker.setLatLng([pos.lat, pos.lon]);
      }
    },
    (e) => { $('gps').textContent = e.code === 1 ? 'Permiso de ubicación denegado' : 'Sin señal GPS'; },
    { enableHighAccuracy: true, maximumAge: 5000, timeout: 30000 }
  );
}

// ---------- Mapa ----------
const ESRI = 'https://server.arcgisonline.com/ArcGIS/rest/services';
const LAYERS = {
  sat: () => [
    L.tileLayer(`${ESRI}/World_Imagery/MapServer/tile/{z}/{y}/{x}`, { maxNativeZoom: 18, maxZoom: 19, crossOrigin: true, attribution: 'Esri, Maxar, Earthstar Geographics' }),
    L.tileLayer(`${ESRI}/Reference/World_Boundaries_and_Places/MapServer/tile/{z}/{y}/{x}`, { maxNativeZoom: 18, maxZoom: 19, crossOrigin: true }),
  ],
  plain: () => [L.tileLayer('https://tile.openstreetmap.org/{z}/{x}/{y}.png', { maxZoom: 19, crossOrigin: true, attribution: '© OpenStreetMap' })],
};
let baseLayers = [], baseKind = 'sat';
function setBase(kind) {
  if (!map) return;
  baseLayers.forEach((l) => map.removeLayer(l));
  baseKind = kind;
  baseLayers = LAYERS[kind]();
  baseLayers[0].on('tileerror', () => ($('mapOff').hidden = false)).on('tileload', () => ($('mapOff').hidden = true));
  baseLayers.forEach((l) => l.addTo(map));
  baseLayers.forEach((l) => l.setZIndex?.(1));
  $('layerBtn').textContent = kind === 'sat' ? 'Mapa: cambiar a plano' : 'Mapa: cambiar a satélite';
  try { localStorage.setItem('pdi_base', kind); } catch {}
}
function initMap() {
  try {
    map = L.map('map', { zoomControl: false, attributionControl: false }).setView([43.2, -2.0], 9);
    markers = L.layerGroup().addTo(map); window.pdiMap = map;
    // Safari cambia el alto útil (barra del navegador) incluso con la vista oculta: reajustar siempre que cambie
    const fit = () => { const el = $('map'); if (map && el.clientWidth && el.clientHeight) map.invalidateSize({ pan: false }); };
    window.pdiFit = () => { fit(); requestAnimationFrame(fit); setTimeout(fit, 120); setTimeout(fit, 450); };
    if ('ResizeObserver' in window) new ResizeObserver(fit).observe($('map'));
    addEventListener('resize', window.pdiFit); addEventListener('orientationchange', window.pdiFit); addEventListener('pageshow', window.pdiFit);
    window.visualViewport?.addEventListener('resize', window.pdiFit);
    let k = 'sat'; try { k = localStorage.getItem('pdi_base') || 'sat'; } catch {}
    setBase(LAYERS[k] ? k : 'sat');
    $('layerBtn').onclick = () => setBase(baseKind === 'sat' ? 'plain' : 'sat');
  } catch { $('mapOff').hidden = false; }
}
function drawMarkers() {
  if (!markers) return;
  markers.clearLayers();
  for (const p of live()) {
    L.circleMarker([p.latitude, p.longitude], { radius: 9, color: '#fff', weight: 2, fillColor: p._dirty ? '#ffd60a' : '#ff453a', fillOpacity: 1 })
      .on('click', () => openSheet(p)).addTo(markers);
  }
}

// ---------- Pintado ----------
function render() {
  const items = live();
  $('count').textContent = items.length ? `(${items.length})` : '';
  $('empty').hidden = items.length > 0;
  const ul = $('list'); ul.textContent = '';
  for (const p of items) {
    const li = document.createElement('li');
    const t = document.createElement('div'); t.className = 't';
    const b = document.createElement('b'); b.textContent = p.name;
    const s = document.createElement('small'); s.textContent = `${p.status === 'visited' ? 'Visitado' : 'Quiero ir'} · ${p.latitude.toFixed(4)}, ${p.longitude.toFixed(4)}`;
    t.append(b, s); li.append(t);
    if (p._dirty) { const x = document.createElement('span'); x.className = 'badge'; x.textContent = 'sin enviar'; li.append(x); }
    li.onclick = () => openSheet(p);
    ul.append(li);
  }
  drawMarkers();
  const pend = pois.filter((p) => p._dirty).length;
  const st = syncing ? 'pend' : pend ? 'pend' : render.err ? 'err' : 'ok';
  const btn = $('syncBtn');
  btn.className = 'cloud ' + st + (syncing ? ' busy' : '');
  btn.setAttribute('aria-label', syncing ? 'Sincronizando…' : pend ? `${pend} sin enviar` : render.err ? 'Error de envío' : 'Al día');
  $('syncN').hidden = !pend; $('syncN').textContent = pend;
}

// ---------- Alta rápida y ficha ----------
// Promedia las lecturas del GPS (ponderadas por precisión) durante unos segundos: mucho más estable que la primera lectura.
async function averagedFix() {
  const t0 = Date.now();
  const window_ = () => recent.filter((r) => r.t >= t0 - 3000);
  while (Date.now() - t0 < 6000) {
    const w = window_();
    if (w.length >= 4 && Math.min(...w.map((r) => r.acc)) <= 5) break;
    await new Promise((r) => setTimeout(r, 300));
  }
  const w = window_(); if (!w.length) return pos;
  const best = Math.max(1, Math.min(...w.map((r) => r.acc)));
  const use = w.filter((r) => r.acc <= best * 2);
  let sw = 0, la = 0, lo = 0;
  for (const r of use) { const k = 1 / Math.max(1, r.acc) ** 2; sw += k; la += r.lat * k; lo += r.lon * k; }
  return { lat: la / sw, lon: lo / sw, acc: best, n: use.length };
}
async function quickSave() {
  if (quickSave.busy) return; quickSave.busy = true;
  const label = $('quick').querySelector('span'), txt = label.textContent;
  try {
    let p0 = null;
    if (pos) { label.textContent = 'Afinando GPS…'; p0 = await averagedFix(); }
    else if (map) p0 = { lat: map.getCenter().lat, lon: map.getCenter().lng, acc: null };
    if (!p0) { toast('Aún no hay posición GPS'); return; }
    const d = new Date();
    const p = {
      id: uuid(), name: `PDI ${d.toLocaleDateString('es')} ${d.toTimeString().slice(0, 5)}`, description: '',
      latitude: +p0.lat.toFixed(6), longitude: +p0.lon.toFixed(6), accuracy_m: p0.acc ? Math.round(p0.acc) : null,
      status: 'want', best_time: '', notes: '', category: 'otro', tags: [], created_at: nowIso(), origin: 'mobile', attachments: [], version: 0,
    };
    await save(p);
    navigator.vibrate?.(40);
    toast(pos ? `Guardado (±${p.accuracy_m} m${p0.n > 1 ? `, media de ${p0.n} lecturas` : ''}). Usa «Ajustar posición» si hace falta.` : 'Guardado en el centro del mapa (sin GPS)', 4000);
    openSheet(p);
  } finally { label.textContent = txt; quickSave.busy = false; }
}

async function openSheet(p) {
  editing = p; pendingPhotos = []; pendingPos = null; photoPos = null; $('fPhotoPos').hidden = true;
  $('fName').value = p.name; $('fDesc').value = p.description || ''; $('fCat').value = p.category || 'otro';
  $('fStatus').value = p.status; $('fBest').value = p.best_time || ''; $('fNotes').value = p.notes || '';
  showCoord();
  await drawPhotos();
  $('sheet').showModal();
}
async function drawPhotos() {
  const box = $('fPhotos'); box.textContent = '';
  for (const a of editing.attachments || []) {
    const att = a.key.split('/').pop().split('.')[0];
    const local = await dbGet('photos', att);
    let blob = local?.blob;
    if (!blob) blob = await fetchRemotePhoto(a.key, att, editing.id);
    if (blob) { const im = new Image(); im.src = URL.createObjectURL(blob); box.append(im); }
  }
  for (const f of pendingPhotos) { const im = new Image(); im.src = URL.createObjectURL(f.blob); box.append(im); }
}
async function fetchRemotePhoto(key, att, poi) {
  const token = await metaGet('token');
  if (!token || !navigator.onLine) return null;
  try {
    const r = await fetch(`${API}/attachments/${encodeURIComponent(key)}`, { headers: { authorization: `Bearer ${token}` } });
    if (!r.ok) return null;
    const blob = await r.blob();
    await dbPut('photos', { att, poi, blob, type: blob.type, uploaded: true }); // caché local
    return blob;
  } catch { return null; }
}
async function shrink(file) { // máx. 1600 px, JPEG 0,8
  const bmp = await createImageBitmap(file);
  const k = Math.min(1, 1600 / Math.max(bmp.width, bmp.height));
  const c = document.createElement('canvas'); c.width = Math.round(bmp.width * k); c.height = Math.round(bmp.height * k);
  c.getContext('2d').drawImage(bmp, 0, 0, c.width, c.height);
  return new Promise((res) => c.toBlob(res, 'image/jpeg', 0.8));
}
$('fFile').onchange = async (e) => {
  for (const f of e.target.files) {
    const g = await exifGps(f); // antes de reducir la foto: al redimensionar se pierde el EXIF
    if (g && !photoPos) { photoPos = g; const b = $('fPhotoPos'); b.textContent = `📍 Usar ubicación de la foto (a ${fmtM(metersBetween(curPos(), g))})`; b.hidden = false; }
    try { pendingPhotos.push({ att: uuid(), blob: await shrink(f) }); } catch { toast('No se pudo leer la foto'); }
  }
  e.target.value = ''; await drawPhotos();
};
$('form').onsubmit = async () => {
  const p = editing; if (!p) return;
  Object.assign(p, {
    name: $('fName').value.trim() || p.name, description: $('fDesc').value, category: $('fCat').value, status: $('fStatus').value,
    best_time: $('fBest').value, notes: $('fNotes').value,
  });
  if (pendingPos) { p.latitude = pendingPos.lat; p.longitude = pendingPos.lon; p.accuracy_m = null; } // posición manual: sin precisión de GPS
  for (const f of pendingPhotos) {
    await dbPut('photos', { att: f.att, poi: p.id, blob: f.blob, type: 'image/jpeg', uploaded: false });
    (p.attachments ||= []).push({ key: `poi/${p.id}/${f.att}.jpg`, type: 'image/jpeg', bytes: f.blob.size });
  }
  await save(p); editing = null;
};
// ---------- Posición: ajuste fino y ubicación de la foto ----------
const curPos = () => pendingPos || { lat: editing.latitude, lon: editing.longitude };
const metersBetween = (a, b) => {
  const k = Math.PI / 180, dLat = (b.lat - a.lat) * k, dLon = (b.lon - a.lon) * k;
  const x = Math.sin(dLat / 2) ** 2 + Math.cos(a.lat * k) * Math.cos(b.lat * k) * Math.sin(dLon / 2) ** 2;
  return 12742000 * Math.asin(Math.sqrt(x));
};
const fmtM = (m) => (m < 1000 ? `${Math.round(m)} m` : `${(m / 1000).toFixed(1)} km`);
function showCoord() {
  const c = curPos();
  $('fCoord').textContent = `${c.lat.toFixed(6)}, ${c.lon.toFixed(6)}${pendingPos ? ' · ajustada a mano' : editing.accuracy_m ? ` · ±${editing.accuracy_m} m` : ''}`;
  $('fGo').href = `https://maps.apple.com/?ll=${c.lat},${c.lon}&q=${encodeURIComponent(editing.name)}`;
}
function exifGps(file) { // lee la ubicación del EXIF de una foto JPEG (sin librerías)
  return file.slice(0, 262144).arrayBuffer().then((buf) => {
    const v = new DataView(buf);
    if (v.getUint16(0) !== 0xffd8) return null;
    for (let o = 2; o + 4 < v.byteLength;) {
      const m = v.getUint16(o), len = v.getUint16(o + 2);
      if (m === 0xffe1 && v.getUint32(o + 4) === 0x45786966) return tiffGps(v, o + 10);
      if ((m & 0xff00) !== 0xff00 || m === 0xffda) break;
      o += 2 + len;
    }
    return null;
  }).catch(() => null);
}
function tiffGps(v, t) {
  const le = v.getUint16(t) === 0x4949, u16 = (p) => v.getUint16(p, le), u32 = (p) => v.getUint32(p, le);
  if (u16(t + 2) !== 42) return null;
  const ifd = (p) => { const out = {}, n = u16(p); for (let i = 0; i < n; i++) { const e = p + 2 + i * 12; out[u16(e)] = e; } return out; };
  const i0 = ifd(t + u32(t + 4));
  if (!i0[0x8825]) return null;
  const g = ifd(t + u32(i0[0x8825] + 8));
  if (!g[1] || !g[2] || !g[3] || !g[4]) return null;
  const deg = (e) => { const p = t + u32(e + 8), r = (k) => u32(p + k * 8) / (u32(p + k * 8 + 4) || 1); return r(0) + r(1) / 60 + r(2) / 3600; };
  const ref = (e) => String.fromCharCode(v.getUint8(e + 8));
  let lat = deg(g[2]), lon = deg(g[4]);
  if (ref(g[1]) === 'S') lat = -lat;
  if (ref(g[3]) === 'W') lon = -lon;
  if (!Number.isFinite(lat) || !Number.isFinite(lon) || (!lat && !lon) || Math.abs(lat) > 90 || Math.abs(lon) > 180) return null;
  return { lat: +lat.toFixed(6), lon: +lon.toFixed(6) };
}
$('fPhotoPos').onclick = () => { pendingPos = { ...photoPos }; $('fPhotoPos').hidden = true; showCoord(); toast('Posición tomada de la foto'); };

let amap, aLayers = [], aSaved, aGpsCircle, aGpsDot, aStep = 1;
const STEPS = [1, 5, 0.5];
function updateAdjust() {
  if (!amap || !editing) return;
  const c = amap.getCenter();
  $('aCoord').textContent = `${c.lat.toFixed(6)}, ${c.lng.toFixed(6)}`;
  const parts = [`${fmtM(metersBetween({ lat: editing.latitude, lon: editing.longitude }, { lat: c.lat, lon: c.lng }))} de la guardada`];
  if (pos) parts.push(`${fmtM(metersBetween(pos, { lat: c.lat, lon: c.lng }))} de tu GPS`);
  $('aDelta').textContent = parts.join(' · ');
}
function updateAdjustGps() {
  if (!amap || !$('adjust').open || !pos) return;
  if (!aGpsCircle) { aGpsCircle = L.circle([pos.lat, pos.lon], { radius: pos.acc, color: '#0a84ff', weight: 1, fillOpacity: 0.12 }).addTo(amap); aGpsDot = L.circleMarker([pos.lat, pos.lon], { radius: 5, color: '#fff', weight: 2, fillColor: '#0a84ff', fillOpacity: 1 }).addTo(amap); }
  else { aGpsCircle.setLatLng([pos.lat, pos.lon]).setRadius(pos.acc); aGpsDot.setLatLng([pos.lat, pos.lon]); }
  updateAdjust();
}
function openAdjust() {
  if (!editing) return;
  const c = curPos();
  $('adjust').showModal();
  if (!amap) {
    amap = L.map('amap', { zoomControl: false, zoomSnap: 0.5, maxZoom: 19 });
    amap.attributionControl.setPrefix(false);
    amap.on('move', updateAdjust);
  }
  aLayers.forEach((l) => amap.removeLayer(l));
  aLayers = LAYERS[baseKind]().map((l) => l.addTo(amap));
  [aSaved, aGpsCircle, aGpsDot].forEach((l) => l && amap.removeLayer(l)); aGpsCircle = aGpsDot = null;
  aSaved = L.circleMarker([editing.latitude, editing.longitude], { radius: 6, color: '#fff', weight: 2, fillOpacity: 0, dashArray: '3 3' }).addTo(amap);
  const go = () => { amap.invalidateSize({ pan: false }); amap.setView([c.lat, c.lon], 19, { animate: false }); updateAdjustGps(); updateAdjust(); };
  go(); setTimeout(go, 80);
}
function nudge(dir) {
  const c = amap.getCenter(), dLat = aStep / 111320, dLon = aStep / (111320 * Math.cos((c.lat * Math.PI) / 180));
  const d = { n: [dLat, 0], s: [-dLat, 0], e: [0, dLon], w: [0, -dLon] }[dir];
  amap.setView([c.lat + d[0], c.lng + d[1]], amap.getZoom(), { animate: false });
}
$('fAdjust').onclick = openAdjust;
document.querySelectorAll('#pad [data-d]').forEach((b) => (b.onclick = () => nudge(b.dataset.d)));
$('aStep').onclick = () => { aStep = STEPS[(STEPS.indexOf(aStep) + 1) % STEPS.length]; $('aStep').textContent = `${aStep} m`; };
$('aGps').onclick = () => { if (!pos) { toast('Aún no hay posición GPS'); return; } amap.setView([pos.lat, pos.lon], amap.getZoom(), { animate: false }); };
$('aCancel').onclick = () => $('adjust').close();
$('aOk').onclick = () => { const c = amap.getCenter(); pendingPos = { lat: +c.lat.toFixed(6), lon: +c.lng.toFixed(6) }; $('adjust').close(); showCoord(); };

$('fCancel').onclick = () => { editing = null; $('sheet').close(); };
$('fDel').onclick = async () => {
  if (!editing || !confirm('¿Borrar este PDI?')) return;
  editing._del = true; await save(editing); $('sheet').close(); editing = null;
};

// ---------- Sincronización ----------
async function api(path, opts = {}) {
  const token = await metaGet('token');
  if (!token) throw Object.assign(new Error('sin sesión'), { auth: true });
  const r = await fetch(API + path, { ...opts, headers: { ...(opts.headers || {}), authorization: `Bearer ${token}` } });
  if (r.status === 401) throw Object.assign(new Error('sesión caducada'), { auth: true });
  return r;
}
async function sync(quiet = false) {
  if (syncing) return;
  if (!navigator.onLine) { if (!quiet) toast('Sin conexión: se enviará al volver la cobertura'); return; }
  syncing = true; render.err = false; render();
  try {
    // 1) Fotos pendientes (antes que los PDI que las citan)
    for (const ph of (await dbAll('photos')).filter((x) => !x.uploaded)) {
      const r = await api(`/attachments?poi=${ph.poi}&att=${ph.att}`, { method: 'PUT', headers: { 'content-type': ph.type || 'image/jpeg' }, body: ph.blob });
      if (!r.ok && r.status !== 201) throw new Error(`foto: ${r.status}`);
      ph.uploaded = true; await dbPut('photos', ph);
    }
    // 2) Cambios locales
    const dirty = pois.filter((p) => p._dirty);
    let resync = false; // tras un conflicto se vuelve a bajar todo para traer la versión del servidor
    for (let i = 0; i < dirty.length; i += 50) {
      const chunk = dirty.slice(i, i + 50);
      const ops = chunk.map((p) => (p._del
        ? { op_id: p._op, type: 'delete', id: p.id, base_version: p.version || 0 }
        : { op_id: p._op, type: 'upsert', base_version: p.version || 0, poi: contract(p) }));
      const r = await api('/sync/push', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ ops }) });
      if (!r.ok) throw new Error(`push: ${r.status}`);
      const { results } = await r.json();
      for (const res of results) {
        const p = chunk.find((x) => x._op === res.op_id); if (!p) continue;
        if (res.status === 'rejected') { p._error = res.reason; await dbPut('pois', p); render.err = true; continue; }
        if (p._op !== res.op_id) continue; // se editó mientras tanto: se enviará de nuevo
        if (res.status === 'conflict') { resync = true; toast('Conflicto: tu versión se guardó como copia «(conflicto)»', 5000); }
        if (p._del) await dbDel('pois', p.id);
        else { p._dirty = false; p.version = res.version || res.server_version || p.version; delete p._error; await dbPut('pois', p); }
      }
    }
    // 3) Novedades del servidor
    let cursor = resync ? 0 : await metaGet('cursor', 0), more = true;
    while (more) {
      const r = await api(`/sync/pull?cursor=${cursor}`);
      if (!r.ok) throw new Error(`pull: ${r.status}`);
      const d = await r.json();
      for (const sp of d.pois) {
        const loc = await dbGet('pois', sp.id);
        if (loc?._dirty) continue; // lo local pendiente se resuelve en el próximo envío
        if (sp.deleted_at) { await dbDel('pois', sp.id); continue; }
        await dbPut('pois', { ...sp, _dirty: false });
      }
      cursor = d.cursor; more = d.has_more; await metaSet('cursor', cursor);
    }
    api('/sync/ack', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ cursor, device: 'mobile' }) }).catch(() => {});
    await metaSet('last_sync', nowIso());
    if (!quiet) toast('Sincronizado');
  } catch (e) {
    render.err = true;
    if (e.auth) { if (!quiet) { toast('Sesión no válida: vincula de nuevo el móvil'); openCfg(); } }
    else if (!quiet) toast(`No se pudo sincronizar (${e.message})`);
  } finally { syncing = false; await reload(); }
}

// ---------- Ajustes / sesión ----------
async function openCfg() {
  const t = await metaGet('token'), last = await metaGet('last_sync');
  const paired = !!(await metaGet('devkey'));
  $('cfgState').textContent = (t ? (paired ? 'Móvil vinculado.' : 'Sesión iniciada.') : 'Sin vincular: los PDI se guardan en el teléfono pero no se envían. Escribe el código de RAW Studio.') + (last ? ` Última sincronización: ${new Date(last).toLocaleString('es')}.` : '');
  const face = !!(await metaGet('cred'));
  $('cfgState').textContent += face ? ' Face ID: activado.' : ' Face ID: sin activar.';
  $('faceBtn').hidden = face || !t;
  $('cfg').showModal();
}
$('cfgBtn').onclick = openCfg;
$('cClose').onclick = () => $('cfg').close();
$('cfgForm').onsubmit = async () => {
  const key = $('cKey').value; $('cKey').value = '';
  if (!key) return;
  try {
    const r = await fetch(`${API}/auth/session`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ key, device: 'mobile' }) });
    if (!r.ok) { toast(r.status === 401 ? 'Clave incorrecta' : `Error ${r.status}`); return; }
    await metaSet('token', (await r.json()).token);
    toast('Sesión iniciada'); sync();
  } catch { toast('Sin conexión: inténtalo con cobertura'); }
};

// ---------- Face ID (WebAuthn) ----------
const LOCK_AFTER_MS = 2 * 60 * 1000; // se vuelve a bloquear tras 2 min en segundo plano
const b64u = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const unb64u = (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4)), (c) => c.charCodeAt(0));
let hiddenAt = 0, unlocking = false;
const setLocked = (v) => { if (v) { $('adjust').close(); $('sheet').close(); $('cfg').close(); } $('lock').hidden = !v; if (!v) window.pdiFit?.(); };

async function enableFaceId() {
  if (!window.PublicKeyCredential) { toast('Este navegador no admite Face ID'); return; }
  try {
    const o = await (await api('/auth/passkey/register/options', { method: 'POST' })).json();
    const cred = await navigator.credentials.create({ publicKey: {
      challenge: unb64u(o.challenge), rp: o.rp, user: { id: unb64u(o.user.id), name: o.user.name, displayName: o.user.displayName },
      pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
      authenticatorSelection: { authenticatorAttachment: 'platform', userVerification: 'required', residentKey: 'preferred' },
      attestation: 'none', timeout: 60000,
    } });
    const r = cred.response;
    const res = await api('/auth/passkey/register', { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id: cred.id, publicKey: b64u(r.getPublicKey()), clientDataJSON: b64u(r.clientDataJSON), label: navigator.platform || 'iPhone' }) });
    if (!res.ok) throw new Error(`servidor ${res.status}`);
    await metaSet('cred', cred.id);
    toast('Face ID activado'); openCfg();
  } catch (e) {
    toast(e.auth ? 'Primero entra con la clave' : e.name === 'NotAllowedError' ? 'Cancelado' : `No se pudo activar Face ID (${e.message})`, 4000);
  }
}

async function unlock() {
  if (unlocking) return; unlocking = true;
  const msg = $('lockMsg'); msg.textContent = '';
  try {
    const cred = await metaGet('cred');
    if (!cred) { setLocked(false); return; }
    let challenge = null;
    if (navigator.onLine) { // con cobertura: el reto lo pone el servidor y la firma renueva la sesión
      try { const r = await fetch(`${API}/auth/passkey/login/options`, { method: 'POST', signal: AbortSignal.timeout(4000) }); if (r.ok) challenge = (await r.json()).challenge; } catch {}
    }
    const a = await navigator.credentials.get({ publicKey: {
      challenge: challenge ? unb64u(challenge) : crypto.getRandomValues(new Uint8Array(32)),
      allowCredentials: [{ type: 'public-key', id: unb64u(cred) }], userVerification: 'required', timeout: 60000,
    } });
    if (!(new Uint8Array(a.response.authenticatorData)[32] & 4)) throw new Error('sin verificación');
    setLocked(false);
    if (challenge) { // sin cobertura el desbloqueo es solo local; los envíos esperan a la próxima vez
      fetch(`${API}/auth/passkey/login`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({
        id: a.id, clientDataJSON: b64u(a.response.clientDataJSON), authenticatorData: b64u(a.response.authenticatorData), signature: b64u(a.response.signature), device: 'mobile' }) })
        .then(async (r) => { if (r.ok && !(await metaGet('devkey'))) await metaSet('token', (await r.json()).token); }) // con clave de dispositivo no hace falta renovar la sesión.catch(() => {}).finally(() => sync(true));
    } else sync(true);
  } catch (e) {
    msg.textContent = e.name === 'NotAllowedError' ? 'Toca el botón para usar Face ID.' : `No se pudo verificar (${e.message}).`;
  } finally { unlocking = false; }
}
$('unlockBtn').onclick = unlock;
$('keyBtn').onclick = async () => { // salida de emergencia: la clave maestra quita el bloqueo de este dispositivo
  const key = prompt('Clave de acceso (quita el bloqueo de este dispositivo)'); if (!key) return;
  try {
    const r = await fetch(`${API}/auth/session`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ key, device: 'mobile' }) });
    if (!r.ok) { $('lockMsg').textContent = r.status === 401 ? 'Clave incorrecta' : `Error ${r.status}`; return; }
    await metaSet('token', (await r.json()).token); await dbDel('meta', 'cred'); setLocked(false); toast('Bloqueo desactivado'); sync(true);
  } catch { $('lockMsg').textContent = 'Necesitas cobertura para usar la clave'; }
};
$('faceBtn').onclick = enableFaceId;

// ---------- Emparejado: el código de RAW Studio se canjea por una clave propia de este móvil ----------
async function claimPair(raw) {
  const code = String(raw || '').toUpperCase().replace(/[^A-Z0-9]/g, '');
  if (code.length !== 8) { toast('El código tiene 8 caracteres'); return false; }
  try {
    const r = await fetch(`${API}/pair/claim`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code, name: navigator.platform || 'móvil' }) });
    if (!r.ok) { toast(r.status === 401 ? 'Código incorrecto o caducado' : r.status === 429 ? 'Demasiados intentos; espera unos minutos' : `Error ${r.status}`, 4000); return false; }
    const { key } = await r.json();
    await metaSet('devkey', key); await metaSet('token', key);
    toast('Móvil vinculado'); sync();
    return true;
  } catch { toast('Sin conexión: inténtalo con cobertura'); return false; }
}
$('pairBtn').onclick = async () => { if (await claimPair($('cCode').value)) { $('cCode').value = ''; $('cfg').close(); } };
$('cCode').onkeydown = (e) => { if (e.key === 'Enter') { e.preventDefault(); $('pairBtn').click(); } };
const isStandalone = () => navigator.standalone === true || matchMedia('(display-mode: standalone)').matches;
let pendingPair = null;
async function pairFromUrl() { // #pair=CODIGO o ?pair=CODIGO
  const m = (location.hash + '&' + location.search).match(/pair=([A-Za-z0-9-]+)/); if (!m) return;
  const strip = () => history.replaceState(null, '', location.pathname);
  const code = m[1].toUpperCase().replace(/[^A-Z0-9]/g, '');
  if (await metaGet('devkey')) { strip(); return; } // ya vinculado: la dirección de arranque guardada al instalar ya no sirve
  if (isStandalone()) { strip(); await claimPair(code); return; } // app instalada: aquí se canjea
  // En Safari NO se canjea (el código es de un solo uso y es para la app instalada): se apunta en el manifiesto
  // para que «Añadir a pantalla de inicio» arranque con el código, y se explica cómo terminar.
  pendingPair = code;
  document.querySelector('link[rel="manifest"]').href = `${API}/manifest?pair=${code}`;
  $('pairBanner').hidden = false;
}
$('pairHere').onclick = async () => { if (pendingPair && await claimPair(pendingPair)) { pendingPair = null; $('pairBanner').hidden = true; history.replaceState(null, '', location.pathname); } };

// ---------- Arranque ----------
const viewTabs = document.querySelectorAll('nav.bar .tab[data-view]'); // solo las pestañas Mapa/Lista (no los botones de la ficha)
viewTabs.forEach((b) => (b.onclick = () => {
  viewTabs.forEach((x) => x.classList.toggle('active', x === b));
  document.querySelectorAll('.view').forEach((v) => v.classList.toggle('active', v.id === b.dataset.view));
  if (b.dataset.view === 'viewMap') window.pdiFit?.();
}));
$('quick').onclick = quickSave;
const resetView = () => {
  if (/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || '')) return;
  scrollTo(0, 0); document.body.scrollTop = 0; document.documentElement.scrollTop = 0;
  window.pdiFit?.();
};
document.addEventListener('focusout', () => setTimeout(resetView, 120));
['sheet', 'cfg', 'adjust'].forEach((id) => $(id).addEventListener('close', () => { setTimeout(resetView, 60); setTimeout(resetView, 400); }));
window.visualViewport?.addEventListener('resize', () => setTimeout(resetView, 150));
$('syncBtn').onclick = () => sync();
{ const bar = document.querySelector('nav.bar'); let t;
  const on = () => { bar.classList.add('on'); clearTimeout(t); };
  const off = () => { clearTimeout(t); t = setTimeout(() => bar.classList.remove('on'), 1200); };
  bar.addEventListener('pointerdown', on); bar.addEventListener('pointerup', off); bar.addEventListener('pointercancel', off); }
addEventListener('online', () => sync(true));
document.addEventListener('visibilitychange', async () => {
  if (document.hidden) { hiddenAt = Date.now(); return; }
  if (hiddenAt && Date.now() - hiddenAt > LOCK_AFTER_MS && (await metaGet('cred'))) { setLocked(true); unlock(); return; }
  sync(true);
});

(async () => {
  initMap(); startGps();
  await reload();
  await pairFromUrl();
  if (await metaGet('cred')) unlock(); else { setLocked(false); if (await metaGet('token')) setTimeout(() => toast('Activa Face ID en ⚙︎', 5000), 1500); }
  navigator.storage?.persist?.(); // pide que iOS no purgue los datos locales
  if ('serviceWorker' in navigator) navigator.serviceWorker.register('sw.js').catch(() => {});
  sync(true);
})();
