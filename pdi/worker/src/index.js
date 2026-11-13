// Light Studio PDI · Worker de Cloudflare (API v1)
// D1 = registro de PDI y cursor · R2 = fotos/adjuntos. Sin dependencias.

const SCHEMA_VERSION = 1;
const MAX_BODY = 1_000_000;          // JSON
const MAX_OPS = 100;                 // operaciones por lote
const MAX_ATTACH = 10 * 1024 * 1024; // 10 MB por adjunto
const SESSION_DAYS = 30;
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const CATEGORIES = ['arbol', 'costa', 'cascada', 'mirador', 'otro'];
const ATT_TYPES = { 'image/jpeg': 'jpg', 'image/png': 'png', 'image/webp': 'webp' };

const json = (obj, status = 200) =>
  new Response(JSON.stringify(obj), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' },
  });
const err = (status, code, message) => json({ error: code, message }, status);

// ---------- Utilidades de seguridad ----------
const enc = new TextEncoder();
const b64url = (buf) =>
  btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

async function sha256(s) {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', enc.encode(s)));
}
async function safeEqual(a, b) {
  const [x, y] = await Promise.all([sha256(a), sha256(b)]);
  let d = 0;
  for (let i = 0; i < x.length; i++) d |= x[i] ^ y[i];
  return d === 0;
}
const hex = (u8) => [...u8].map((x) => x.toString(16).padStart(2, '0')).join('');
async function hmac(secret, msg) {
  const key = await crypto.subtle.importKey('raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return b64url(await crypto.subtle.sign('HMAC', key, enc.encode(msg)));
}
async function makeSession(env, device) {
  const exp = Math.floor(Date.now() / 1000) + SESSION_DAYS * 86400;
  const body = `${exp}.${device}`;
  return { token: `s.${body}.${await hmac(env.SESSION_SECRET, body)}`, expires_at: new Date(exp * 1000).toISOString() };
}
async function authorized(request, env) {
  const h = request.headers.get('authorization') || '';
  if (!h.startsWith('Bearer ')) return null;
  const tok = h.slice(7).trim();
  if (!env.API_KEY || !env.SESSION_SECRET) return null;
  if (tok.startsWith('s.')) {
    const parts = tok.split('.');
    if (parts.length < 4) return null;
    const sig = parts.pop();
    const body = parts.slice(1).join('.');
    const exp = Number(parts[1]);
    if (!Number.isFinite(exp) || exp < Date.now() / 1000) return null;
    if (!(await safeEqual(sig, await hmac(env.SESSION_SECRET, body)))) return null;
    return { device: parts.slice(2).join('.') || 'mobile' };
  }
  if (await safeEqual(tok, env.API_KEY)) return { device: request.headers.get('x-device') || 'api', master: true };
  if (tok.startsWith('pdi_') && tok.length <= 80) { // clave de un dispositivo emparejado (revocable)
    const row = await env.DB.prepare('SELECT id, name, last_used FROM device_keys WHERE key_hash = ?1 AND revoked = 0').bind(hex(await sha256(tok))).first();
    if (row) {
      if (!row.last_used || Date.parse(row.last_used) < Date.now() - 600000) {
        await env.DB.prepare('UPDATE device_keys SET last_used = ?1 WHERE id = ?2').bind(new Date().toISOString(), row.id).run();
      }
      return { device: row.name, key_id: row.id };
    }
  }
  return null;
}

async function readJson(request) {
  const len = Number(request.headers.get('content-length') || 0);
  if (len > MAX_BODY) throw new HttpError(413, 'too_large', 'Cuerpo demasiado grande');
  const text = await request.text();
  if (text.length > MAX_BODY) throw new HttpError(413, 'too_large', 'Cuerpo demasiado grande');
  try {
    return JSON.parse(text);
  } catch {
    throw new HttpError(400, 'bad_json', 'JSON no válido');
  }
}
class HttpError extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

// ---------- Validación del PDI (contrato v1) ----------
const str = (v, max) => (typeof v === 'string' ? v.trim().slice(0, max) : '');
const isIso = (v) => typeof v === 'string' && !Number.isNaN(Date.parse(v));

function normalizePoi(p, now) {
  if (!p || typeof p !== 'object') throw new Error('PDI vacío');
  if (!UUID_RE.test(p.id || '')) throw new Error('id no es un UUID');
  const name = str(p.name, 120);
  if (!name) throw new Error('name obligatorio');
  const lat = Number(p.latitude);
  const lon = Number(p.longitude);
  if (!(lat >= -90 && lat <= 90) || !(lon >= -180 && lon <= 180)) throw new Error('coordenadas fuera de rango');
  const tags = Array.isArray(p.tags) ? p.tags.map((t) => str(t, 40)).filter(Boolean).slice(0, 20) : [];
  const attachments = [];
  for (const a of Array.isArray(p.attachments) ? p.attachments.slice(0, 10) : []) {
    if (!a || typeof a.key !== 'string' || !a.key.startsWith(`poi/${p.id.toLowerCase()}/`)) continue;
    if (!ATT_TYPES[a.type]) continue;
    attachments.push({ key: a.key, type: a.type, bytes: Math.max(0, Number(a.bytes) || 0) });
  }
  return {
    schema_version: SCHEMA_VERSION,
    id: p.id.toLowerCase(),
    name,
    description: str(p.description, 2000),
    latitude: lat,
    longitude: lon,
    status: p.status === 'visited' ? 'visited' : 'want',
    best_time: str(p.best_time, 200),
    notes: str(p.notes, 2000),
    category: CATEGORIES.includes(p.category) ? p.category : 'otro',
    tags,
    created_at: isIso(p.created_at) ? new Date(p.created_at).toISOString() : now,
    origin: p.origin === 'desktop' ? 'desktop' : 'mobile',
    attachments,
    accuracy_m: p.accuracy_m != null && Number.isFinite(Number(p.accuracy_m)) ? Math.max(0, Number(p.accuracy_m)) : null,
  };
}

const publicPoi = (row) => ({
  ...JSON.parse(row.data),
  version: row.version,
  seq: row.seq,
  updated_at: row.updated_at,
  deleted_at: row.deleted_at,
});

// ---------- Sincronización ----------
// Cada cambio aceptado: sube el contador, escribe el PDI con ese seq y deja constancia de la operación,
// todo en un único lote (transacción) para que un fallo a medias no deje huecos.
async function commit(env, { id, data, version, deleted_at, opId, result, now }) {
  const stmts = [
    env.DB.prepare('UPDATE seq_counter SET value = value + 1 WHERE id = 1'),
    env.DB.prepare(
      `INSERT INTO pois (id, seq, version, data, updated_at, deleted_at)
       VALUES (?1, (SELECT value FROM seq_counter WHERE id = 1), ?2, ?3, ?4, ?5)
       ON CONFLICT(id) DO UPDATE SET seq = excluded.seq, version = excluded.version,
         data = excluded.data, updated_at = excluded.updated_at, deleted_at = excluded.deleted_at`
    ).bind(id, version, JSON.stringify(data), now, deleted_at),
  ];
  if (opId) {
    stmts.push(
      env.DB.prepare('INSERT INTO ops (op_id, result, created_at) VALUES (?1, ?2, ?3)').bind(opId, JSON.stringify(result), now)
    );
  }
  await env.DB.batch(stmts);
}

async function applyOp(env, op) {
  const now = new Date().toISOString();
  if (!op || typeof op !== 'object') return { status: 'rejected', reason: 'operación vacía' };
  const opId = typeof op.op_id === 'string' && op.op_id.length >= 8 && op.op_id.length <= 64 ? op.op_id : null;
  if (!opId) return { status: 'rejected', reason: 'op_id obligatorio (8-64 caracteres)' };

  const seen = await env.DB.prepare('SELECT result FROM ops WHERE op_id = ?1').bind(opId).first();
  if (seen) return { ...JSON.parse(seen.result), op_id: opId, status: 'duplicate' };

  const baseVersion = Number.isInteger(op.base_version) ? op.base_version : 0;
  let result;

  if (op.type === 'upsert') {
    let poi;
    try {
      poi = normalizePoi(op.poi, now);
    } catch (e) {
      // Las operaciones rechazadas no se registran: el cliente puede corregirlas y reenviarlas.
      return { op_id: opId, status: 'rejected', reason: e.message };
    }
    const cur = await env.DB.prepare('SELECT * FROM pois WHERE id = ?1').bind(poi.id).first();
    if (!cur) {
      await commit(env, { id: poi.id, data: poi, version: 1, deleted_at: null, opId, result: { id: poi.id, version: 1, status: 'accepted' }, now });
      result = { id: poi.id, version: 1, status: 'accepted' };
    } else if (cur.version === baseVersion && !cur.deleted_at) {
      const version = cur.version + 1;
      await commit(env, { id: poi.id, data: poi, version, deleted_at: null, opId, result: { id: poi.id, version, status: 'accepted' }, now });
      result = { id: poi.id, version, status: 'accepted' };
    } else {
      // Conflicto: el servidor conserva lo suyo y la versión del cliente se guarda como copia marcada.
      const copyId = crypto.randomUUID();
      const copy = {
        ...poi,
        id: copyId,
        name: `${poi.name} (conflicto)`,
        tags: [...new Set([...poi.tags, 'conflicto'])],
        conflict_of: poi.id,
      };
      // La copia no puede reutilizar las claves de adjuntos del original ya guardadas bajo otro id: se mantienen tal cual.
      result = { id: poi.id, status: 'conflict', server_version: cur.version, copy_id: copyId };
      await commit(env, { id: copyId, data: copy, version: 1, deleted_at: null, opId, result, now });
    }
  } else if (op.type === 'delete') {
    const id = String(op.id || '').toLowerCase();
    if (!UUID_RE.test(id)) return { op_id: opId, status: 'rejected', reason: 'id no es un UUID' };
    const cur = await env.DB.prepare('SELECT * FROM pois WHERE id = ?1').bind(id).first();
    if (!cur || cur.deleted_at) {
      result = { id, status: 'accepted', version: cur ? cur.version : 0 };
      await env.DB.prepare('INSERT INTO ops (op_id, result, created_at) VALUES (?1, ?2, ?3)').bind(opId, JSON.stringify(result), now).run();
    } else if (cur.version !== baseVersion) {
      result = { id, status: 'conflict', server_version: cur.version };
      await env.DB.prepare('INSERT INTO ops (op_id, result, created_at) VALUES (?1, ?2, ?3)').bind(opId, JSON.stringify(result), now).run();
    } else {
      const version = cur.version + 1;
      result = { id, version, status: 'accepted' };
      await commit(env, { id, data: JSON.parse(cur.data), version, deleted_at: now, opId, result, now });
    }
  } else {
    return { op_id: opId, status: 'rejected', reason: 'type debe ser upsert o delete' };
  }
  return { ...result, op_id: opId };
}

async function push(request, env) {
  const body = await readJson(request);
  const ops = Array.isArray(body.ops) ? body.ops : null;
  if (!ops) throw new HttpError(400, 'bad_request', 'Falta ops[]');
  if (ops.length > MAX_OPS) throw new HttpError(413, 'too_many_ops', `Máximo ${MAX_OPS} operaciones por lote`);
  const results = [];
  for (const op of ops) results.push(await applyOp(env, op)); // en orden: una op puede depender de la anterior
  const row = await env.DB.prepare('SELECT value FROM seq_counter WHERE id = 1').first();
  return json({ results, cursor: row ? row.value : 0 });
}

async function pull(url, env) {
  const cursor = Math.max(0, parseInt(url.searchParams.get('cursor') || '0', 10) || 0);
  const limit = Math.min(500, Math.max(1, parseInt(url.searchParams.get('limit') || '200', 10) || 200));
  const { results } = await env.DB.prepare('SELECT * FROM pois WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2').bind(cursor, limit + 1).all();
  const more = results.length > limit;
  const rows = more ? results.slice(0, limit) : results;
  const last = rows.length ? rows[rows.length - 1].seq : cursor;
  return json({ pois: rows.map(publicPoi), cursor: last, has_more: more });
}

async function ack(request, env, who) {
  const b = await readJson(request);
  const cursor = parseInt(b.cursor, 10);
  if (!Number.isInteger(cursor) || cursor < 0) throw new HttpError(400, 'bad_request', 'cursor no válido');
  const device = str(b.device, 40) || who.device;
  await env.DB.prepare(
    `INSERT INTO devices (device, cursor, updated_at) VALUES (?1, ?2, ?3)
     ON CONFLICT(device) DO UPDATE SET cursor = MAX(cursor, excluded.cursor), updated_at = excluded.updated_at`
  ).bind(device, cursor, new Date().toISOString()).run();
  return json({ ok: true, device, cursor });
}

// ---------- Adjuntos (R2) ----------
async function putAttachment(request, url, env) {
  const poi = (url.searchParams.get('poi') || '').toLowerCase();
  const att = (url.searchParams.get('att') || crypto.randomUUID()).toLowerCase();
  if (!UUID_RE.test(poi) || !UUID_RE.test(att)) throw new HttpError(400, 'bad_request', 'poi y att deben ser UUID');
  const type = (request.headers.get('content-type') || '').split(';')[0].trim();
  const ext = ATT_TYPES[type];
  if (!ext) throw new HttpError(415, 'bad_type', 'Solo image/jpeg, image/png o image/webp');
  const len = Number(request.headers.get('content-length') || 0);
  if (len > MAX_ATTACH) throw new HttpError(413, 'too_large', 'Adjunto de más de 10 MB');
  const buf = await request.arrayBuffer();
  if (buf.byteLength === 0 || buf.byteLength > MAX_ATTACH) throw new HttpError(413, 'too_large', 'Tamaño de adjunto no válido');
  const key = `poi/${poi}/${att}.${ext}`;
  await env.FILES.put(key, buf, { httpMetadata: { contentType: type } });
  await env.DB.prepare(
    `INSERT INTO attachments (key, poi_id, type, bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
     ON CONFLICT(key) DO UPDATE SET bytes = excluded.bytes, type = excluded.type`
  ).bind(key, poi, type, buf.byteLength, new Date().toISOString()).run();
  return json({ key, type, bytes: buf.byteLength }, 201);
}

async function getAttachment(key, env) {
  if (!/^poi\/[0-9a-f-]{36}\/[0-9a-f-]{36}\.(jpg|png|webp)$/.test(key)) throw new HttpError(400, 'bad_key', 'Clave no válida');
  const obj = await env.FILES.get(key);
  if (!obj) return err(404, 'not_found', 'Adjunto inexistente');
  return new Response(obj.body, {
    headers: {
      'content-type': obj.httpMetadata?.contentType || 'application/octet-stream',
      'cache-control': 'private, max-age=31536000, immutable',
      etag: obj.httpEtag,
    },
  });
}

// ---------- Passkeys (Face ID / WebAuthn) ----------
const unb64u = (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4)), (c) => c.charCodeAt(0));
const concat = (a, b) => { const o = new Uint8Array(a.length + b.length); o.set(a); o.set(b, a.length); return o; };

async function newChallenge(env, kind) {
  await env.DB.prepare('DELETE FROM challenges WHERE expires < ?1').bind(Date.now()).run();
  const n = await env.DB.prepare('SELECT COUNT(*) AS n FROM challenges').first();
  if (n.n >= 50) throw new HttpError(429, 'busy', 'Demasiados intentos; espera un minuto');
  const challenge = b64url(crypto.getRandomValues(new Uint8Array(32)));
  await env.DB.prepare('INSERT INTO challenges (challenge, kind, expires) VALUES (?1, ?2, ?3)').bind(challenge, kind, Date.now() + 120000).run();
  return challenge;
}
async function takeChallenge(env, challenge, kind) { // un solo uso
  if (typeof challenge !== 'string') return false;
  const r = await env.DB.prepare('DELETE FROM challenges WHERE challenge = ?1 AND kind = ?2 AND expires >= ?3').bind(challenge, kind, Date.now()).run();
  return r.meta.changes === 1;
}
function parseClientData(b64, type, origin) {
  if (typeof b64 !== 'string') throw new HttpError(400, 'bad_request', 'Falta clientDataJSON');
  const raw = unb64u(b64);
  let cd;
  try { cd = JSON.parse(new TextDecoder().decode(raw)); } catch { throw new HttpError(400, 'bad_request', 'clientDataJSON no válido'); }
  if (cd.type !== type || cd.origin !== origin) throw new HttpError(401, 'bad_passkey', 'Origen o tipo no válidos');
  return { cd, raw };
}
const importSpki = (spki) => crypto.subtle.importKey('spki', spki, { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']);
function derToRaw(der) { // firma ECDSA DER -> r||s (64 bytes)
  if (der[0] !== 0x30) throw new Error('DER');
  let i = 2; if (der[1] & 0x80) i = 2 + (der[1] & 0x7f);
  const read = () => {
    if (der[i++] !== 0x02) throw new Error('DER');
    let len = der[i++]; let v = der.slice(i, i + len); i += len;
    while (v.length > 32 && v[0] === 0) v = v.slice(1);
    if (v.length > 32) throw new Error('DER');
    const out = new Uint8Array(32); out.set(v, 32 - v.length); return out;
  };
  const r = read(), s = read();
  return concat(r, s);
}

async function passkeyRegisterOptions(request, env) {
  const url = new URL(request.url);
  return json({
    challenge: await newChallenge(env, 'register'),
    rp: { id: url.hostname, name: 'Light Studio PDI' },
    user: { id: b64url(enc.encode('luis')), name: 'luis', displayName: 'Luis' },
  });
}
async function passkeyRegister(request, env) { // exige sesión/clave: así solo tú puedes añadir dispositivos
  const b = await readJson(request);
  const { cd } = parseClientData(b.clientDataJSON, 'webauthn.create', new URL(request.url).origin);
  if (!(await takeChallenge(env, cd.challenge, 'register'))) throw new HttpError(401, 'bad_challenge', 'Reto caducado o repetido');
  if (typeof b.id !== 'string' || !/^[\w-]{16,512}$/.test(b.id) || typeof b.publicKey !== 'string') throw new HttpError(400, 'bad_request', 'Credencial no válida');
  try { await importSpki(unb64u(b.publicKey)); } catch { throw new HttpError(400, 'bad_request', 'Clave pública no válida (se espera ES256)'); }
  await env.DB.prepare(
    `INSERT INTO credentials (id, public_key, label, created_at) VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(id) DO UPDATE SET public_key = excluded.public_key, label = excluded.label`
  ).bind(b.id, b.publicKey, str(b.label, 60), new Date().toISOString()).run();
  return json({ ok: true }, 201);
}
async function passkeyLoginOptions(request, env) {
  const { results } = await env.DB.prepare('SELECT id FROM credentials').all();
  return json({ challenge: await newChallenge(env, 'login'), rp_id: new URL(request.url).hostname, credentials: results.map((r) => r.id) });
}
async function passkeyLogin(request, env) {
  const fail = async (msg) => { await new Promise((r) => setTimeout(r, 400)); throw new HttpError(401, 'bad_passkey', msg); };
  const b = await readJson(request);
  const url = new URL(request.url);
  const { cd, raw } = parseClientData(b.clientDataJSON, 'webauthn.get', url.origin);
  if (!(await takeChallenge(env, cd.challenge, 'login'))) return fail('Reto caducado o repetido');
  const cred = await env.DB.prepare('SELECT * FROM credentials WHERE id = ?1').bind(String(b.id || '')).first();
  if (!cred) return fail('Dispositivo no registrado');
  let ok = false;
  try {
    const ad = unb64u(String(b.authenticatorData || ''));
    const rpHash = new Uint8Array(await crypto.subtle.digest('SHA-256', enc.encode(url.hostname)));
    const sameRp = ad.length >= 37 && rpHash.every((v, i) => v === ad[i]);
    const flagsOk = ad.length >= 37 && (ad[32] & 0x01) && (ad[32] & 0x04); // presencia + verificación (Face ID)
    if (sameRp && flagsOk) {
      const clientHash = new Uint8Array(await crypto.subtle.digest('SHA-256', raw));
      ok = await crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, await importSpki(unb64u(cred.public_key)), derToRaw(unb64u(String(b.signature || ''))), concat(ad, clientHash));
    }
  } catch { ok = false; }
  if (!ok) return fail('Firma no válida');
  await env.DB.prepare('UPDATE credentials SET last_used = ?1 WHERE id = ?2').bind(new Date().toISOString(), cred.id).run();
  return json(await makeSession(env, str(b.device, 40).replace(/[^\w-]/g, '') || 'mobile'));
}

// ---------- Emparejado de dispositivos ----------
// La clave maestra (API_KEY) pide un código de un solo uso (10 min); el dispositivo lo canjea por su propia clave, revocable.
const PAIR_ALPHABET = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'; // sin 0/O/1/I/L
const PAIR_TTL_MS = 10 * 60 * 1000;
const normCode = (c) => String(c || '').toUpperCase().replace(/[^A-Z0-9]/g, '');
const codeHash = async (c) => hex(await sha256(`pair:${c}`));

async function pairCreate(request, env) {
  await env.DB.prepare('DELETE FROM pair_codes WHERE expires < ?1').bind(Date.now()).run();
  const n = await env.DB.prepare('SELECT COUNT(*) AS n FROM pair_codes').first();
  if (n.n >= 10) throw new HttpError(429, 'busy', 'Demasiados códigos activos; espera a que caduquen');
  let code = '';
  for (const x of crypto.getRandomValues(new Uint8Array(8))) code += PAIR_ALPHABET[x % PAIR_ALPHABET.length];
  const expires = Date.now() + PAIR_TTL_MS;
  await env.DB.prepare('INSERT INTO pair_codes (code_hash, expires) VALUES (?1, ?2)').bind(await codeHash(code), expires).run();
  return json({ code: `${code.slice(0, 4)}-${code.slice(4)}`, expires_at: new Date(expires).toISOString(), link: `${new URL(request.url).origin}/#pair=${code}` }, 201);
}

async function pairClaim(request, env) {
  const b = await readJson(request);
  await env.DB.prepare('DELETE FROM pair_fails WHERE at < ?1').bind(Date.now() - PAIR_TTL_MS).run();
  const f = await env.DB.prepare('SELECT COUNT(*) AS n FROM pair_fails').first();
  if (f.n >= 20) throw new HttpError(429, 'busy', 'Demasiados intentos; espera unos minutos');
  const code = normCode(b.code);
  let ok = false;
  if (code.length === 8) {
    const r = await env.DB.prepare('DELETE FROM pair_codes WHERE code_hash = ?1 AND expires >= ?2').bind(await codeHash(code), Date.now()).run();
    ok = r.meta.changes === 1; // un solo uso: el canje borra el código
  }
  if (!ok) {
    await env.DB.prepare('INSERT INTO pair_fails (at) VALUES (?1)').bind(Date.now()).run();
    await new Promise((r) => setTimeout(r, 400));
    throw new HttpError(401, 'bad_code', 'Código incorrecto o caducado');
  }
  const name = str(b.name, 40).replace(/[^\w .-]/g, '').trim() || 'dispositivo';
  const key = `pdi_${b64url(crypto.getRandomValues(new Uint8Array(32)))}`;
  const id = crypto.randomUUID();
  await env.DB.prepare('INSERT INTO device_keys (id, name, key_hash, created_at) VALUES (?1, ?2, ?3, ?4)').bind(id, name, hex(await sha256(key)), new Date().toISOString()).run();
  return json({ id, name, key }, 201);
}

// Manifiesto dinámico: al instalar la PWA desde Safari, su start_url lleva el código para que la app instalada se vincule sola.
// No consulta ni gasta el código: solo lo copia (si tiene el formato correcto) en la dirección de arranque.
function dynamicManifest(url) {
  const code = normCode(url.searchParams.get('pair'));
  const start = code.length === 8 ? `/?pair=${code}` : '/';
  return new Response(JSON.stringify({
    id: '/', name: 'Light Studio PDI', short_name: 'PDI', description: 'Puntos de interés fotográficos, también sin cobertura', lang: 'es',
    start_url: start, scope: '/', display: 'standalone', orientation: 'portrait', background_color: '#ffffff', theme_color: '#ffffff',
    icons: [
      { src: '/icons/icon-192.png', sizes: '192x192', type: 'image/png' },
      { src: '/icons/icon-512.png', sizes: '512x512', type: 'image/png' },
      { src: '/icons/icon-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
    ],
  }), { headers: { 'content-type': 'application/manifest+json; charset=utf-8', 'cache-control': 'no-store' } });
}

async function listKeys(env) {
  const { results } = await env.DB.prepare('SELECT id, name, created_at, last_used, revoked FROM device_keys ORDER BY created_at DESC').all();
  return json({ devices: results.map((r) => ({ ...r, revoked: !!r.revoked })) });
}
async function revokeKey(id, env) {
  const r = await env.DB.prepare('UPDATE device_keys SET revoked = 1 WHERE id = ?1').bind(id).run();
  if (r.meta.changes !== 1) throw new HttpError(404, 'not_found', 'Dispositivo desconocido');
  return json({ ok: true, id });
}

// ---------- Rutas (del Mac al móvil) ----------
const MAX_ROUTE_POINTS = 5000;
const MAX_ROUTE_WAYPOINTS = 200;
const finite = (v) => typeof v === 'number' && Number.isFinite(v);
const okPlace = (la, lo) => finite(la) && finite(lo) && Math.abs(la) <= 90 && Math.abs(lo) <= 180;
const total = (v) => (finite(v) && v >= 0 && v < 1e8 ? Math.round(v) : 0);

function normalizeRoute(r) {
  if (!r || typeof r !== 'object' || typeof r.id !== 'string' || !UUID_RE.test(r.id)) throw new HttpError(400, 'bad_route', 'La ruta necesita un id (UUID)');
  const points = [];
  for (const p of Array.isArray(r.points) ? r.points : []) {
    if (points.length >= MAX_ROUTE_POINTS) break;
    if (!Array.isArray(p) || !okPlace(p[0], p[1])) continue;
    points.push(finite(p[2]) ? [p[0], p[1], Math.round(p[2])] : [p[0], p[1]]);
  }
  if (points.length < 2) throw new HttpError(400, 'bad_route', 'La ruta necesita al menos dos puntos');
  const waypoints = [];
  for (const w of Array.isArray(r.waypoints) ? r.waypoints : []) {
    if (waypoints.length >= MAX_ROUTE_WAYPOINTS) break;
    if (w && okPlace(w.lat, w.lon)) waypoints.push({ name: str(w.name, 120), lat: w.lat, lon: w.lon });
  }
  return {
    id: r.id.toLowerCase(),
    name: str(r.name, 120) || 'Ruta',
    distance_m: total(r.distance_m),
    ascent_m: total(r.ascent_m),
    descent_m: total(r.descent_m),
    data: JSON.stringify({ points, waypoints }),
  };
}

async function putRoute(request, env) {
  const r = normalizeRoute(await readJson(request));
  const now = new Date().toISOString();
  await env.DB.prepare(
    'INSERT INTO routes (id, name, distance_m, ascent_m, descent_m, data, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ' +
      'ON CONFLICT(id) DO UPDATE SET name=?2, distance_m=?3, ascent_m=?4, descent_m=?5, data=?6, updated_at=?7',
  ).bind(r.id, r.name, r.distance_m, r.ascent_m, r.descent_m, r.data, now).run();
  return json({ ok: true, id: r.id }, 201);
}

async function listRoutes(env) {
  const { results } = await env.DB.prepare('SELECT id, name, distance_m, ascent_m, descent_m, updated_at FROM routes ORDER BY updated_at DESC LIMIT 200').all();
  return json({ routes: results });
}

async function getRoute(id, env) {
  const row = await env.DB.prepare('SELECT id, name, distance_m, ascent_m, descent_m, data, updated_at FROM routes WHERE id = ?1').bind(id).first();
  if (!row) return err(404, 'not_found', 'Esa ruta no está en el Worker');
  const { data, ...rest } = row;
  return json({ ...rest, ...JSON.parse(data) });
}

async function deleteRoute(request, env) {
  const b = await readJson(request);
  if (typeof b.id !== 'string' || !UUID_RE.test(b.id)) throw new HttpError(400, 'bad_route', 'Falta el id de la ruta');
  await env.DB.prepare('DELETE FROM routes WHERE id = ?1').bind(b.id.toLowerCase()).run(); // si no estaba, tampoco es un error
  return json({ ok: true, id: b.id.toLowerCase() });
}

// ---------- Enrutado ----------
export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname;
    if (!path.startsWith('/api/')) {
      return env.ASSETS ? env.ASSETS.fetch(request) : err(404, 'not_found', 'No encontrado');
    }
    try {
      const m = request.method;
      if (path === '/api/v1/health' && m === 'GET') return json({ ok: true, schema_version: SCHEMA_VERSION, time: new Date().toISOString() });

      if (path === '/api/v1/auth/session' && m === 'POST') {
        const b = await readJson(request);
        if (!env.API_KEY || !env.SESSION_SECRET) return err(500, 'not_configured', 'Faltan secretos en el Worker');
        if (typeof b.key !== 'string' || !(await safeEqual(b.key, env.API_KEY))) {
          await new Promise((r) => setTimeout(r, 400)); // frena la fuerza bruta
          return err(401, 'bad_key', 'Clave incorrecta');
        }
        const device = str(b.device, 40).replace(/[^\w-]/g, '') || 'mobile';
        return json(await makeSession(env, device));
      }

      if (path === '/api/v1/auth/passkey/login/options' && m === 'POST') return await passkeyLoginOptions(request, env);
      if (path === '/api/v1/auth/passkey/login' && m === 'POST') return await passkeyLogin(request, env);
      if (path === '/api/v1/pair/claim' && m === 'POST') return await pairClaim(request, env);
      if (path === '/api/v1/manifest' && m === 'GET') return dynamicManifest(url);

      const who = await authorized(request, env);
      if (!who) return err(401, 'unauthorized', 'Falta autenticación');

      if (path === '/api/v1/pair' || path === '/api/v1/devices' || path.startsWith('/api/v1/devices/')) { // administración: solo la clave maestra
        if (!who.master) return err(403, 'forbidden', 'Solo la clave maestra puede hacer esto');
        if (path === '/api/v1/pair' && m === 'POST') return await pairCreate(request, env);
        if (path === '/api/v1/devices' && m === 'GET') return await listKeys(env);
        const mm = path.match(/^\/api\/v1\/devices\/([0-9a-f-]{36})\/revoke$/i);
        if (mm && m === 'POST') return await revokeKey(mm[1].toLowerCase(), env);
      }
      if (path === '/api/v1/auth/passkey/register/options' && m === 'POST') return await passkeyRegisterOptions(request, env);
      if (path === '/api/v1/auth/passkey/register' && m === 'POST') return await passkeyRegister(request, env);
      if (path === '/api/v1/sync/push' && m === 'POST') return await push(request, env);
      if (path === '/api/v1/sync/pull' && m === 'GET') return await pull(url, env);
      if (path === '/api/v1/sync/ack' && m === 'POST') return await ack(request, env, who);
      if (path === '/api/v1/routes' && m === 'POST') return await putRoute(request, env);
      if (path === '/api/v1/routes' && m === 'GET') return await listRoutes(env);
      if (path === '/api/v1/routes/delete' && m === 'POST') return await deleteRoute(request, env);
      const rm = path.match(/^\/api\/v1\/routes\/([0-9a-f-]{36})$/i);
      if (rm && m === 'GET') return await getRoute(rm[1].toLowerCase(), env);
      if (path === '/api/v1/attachments' && (m === 'POST' || m === 'PUT')) return await putAttachment(request, url, env);
      if (path.startsWith('/api/v1/attachments/') && m === 'GET') return await getAttachment(decodeURIComponent(path.slice('/api/v1/attachments/'.length)), env);

      return err(404, 'not_found', 'Ruta desconocida');
    } catch (e) {
      if (e instanceof HttpError) return err(e.status, e.code, e.message);
      console.error(e);
      return err(500, 'internal', 'Error interno');
    }
  },
};
