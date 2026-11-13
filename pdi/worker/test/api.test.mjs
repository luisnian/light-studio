// Pruebas contra un Worker en marcha: BASE=http://localhost:8799 KEY=... node --test test/
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';

const BASE = process.env.BASE || 'http://localhost:8799';
const KEY = process.env.KEY || 'clave-de-prueba-123';
const A = `${BASE}/api/v1`;
const H = { authorization: `Bearer ${KEY}`, 'content-type': 'application/json' };
const post = (p, b, h = H) => fetch(A + p, { method: 'POST', headers: h, body: JSON.stringify(b) });
const poi = (o = {}) => ({ id: randomUUID(), name: 'Roble', latitude: 43.1, longitude: -2.1, ...o });
const up = (p, base = 0, op = randomUUID()) => ({ op_id: op, type: 'upsert', base_version: base, poi: p });
const pushOne = async (op) => (await (await post('/sync/push', { ops: [op] })).json()).results[0];
const pullAll = async (cursor = 0) => (await (await fetch(`${A}/sync/pull?cursor=${cursor}&limit=500`, { headers: H })).json());

test('health sin auth; API exige token', async () => {
  assert.equal((await fetch(`${A}/health`)).status, 200);
  assert.equal((await fetch(`${A}/sync/pull`)).status, 401);
  assert.equal((await fetch(`${A}/sync/pull`, { headers: { authorization: 'Bearer mala' } })).status, 401);
});

test('sesión: clave mala 401; buena devuelve token que funciona', async () => {
  assert.equal((await post('/auth/session', { key: 'no' }, { 'content-type': 'application/json' })).status, 401);
  const r = await post('/auth/session', { key: KEY, device: 'mobile' }, { 'content-type': 'application/json' });
  assert.equal(r.status, 200);
  const { token } = await r.json();
  assert.ok(token.startsWith('s.'));
  assert.equal((await fetch(`${A}/sync/pull`, { headers: { authorization: `Bearer ${token}` } })).status, 200);
  assert.equal((await fetch(`${A}/sync/pull`, { headers: { authorization: `Bearer ${token.slice(0, -2)}xx` } })).status, 401);
});

test('alta idempotente: el mismo op_id repetido no duplica', async () => {
  const p = poi(); const op = randomUUID();
  const r1 = await pushOne(up(p, 0, op));
  assert.equal(r1.status, 'accepted'); assert.equal(r1.version, 1);
  const r2 = await pushOne(up(p, 0, op));
  assert.equal(r2.status, 'duplicate');
  const mine = (await pullAll()).pois.filter((x) => x.id === p.id);
  assert.equal(mine.length, 1); assert.equal(mine[0].version, 1);
});

test('edición con versión correcta sube versión y seq', async () => {
  const p = poi(); await pushOne(up(p));
  const before = (await pullAll()).pois.find((x) => x.id === p.id);
  const r = await pushOne(up({ ...p, name: 'Roble grande' }, 1));
  assert.equal(r.status, 'accepted'); assert.equal(r.version, 2);
  const after = (await pullAll(before.seq)).pois.find((x) => x.id === p.id);
  assert.equal(after.name, 'Roble grande'); assert.ok(after.seq > before.seq);
});

test('conflicto: versión vieja no pisa; queda copia marcada', async () => {
  const p = poi(); await pushOne(up(p));
  await pushOne(up({ ...p, name: 'Desde el Mac' }, 1));
  const r = await pushOne(up({ ...p, name: 'Desde el móvil' }, 1));
  assert.equal(r.status, 'conflict'); assert.equal(r.server_version, 2); assert.ok(r.copy_id);
  const all = (await pullAll()).pois;
  assert.equal(all.find((x) => x.id === p.id).name, 'Desde el Mac');
  const copy = all.find((x) => x.id === r.copy_id);
  assert.match(copy.name, /Desde el móvil \(conflicto\)/); assert.ok(copy.tags.includes('conflicto')); assert.equal(copy.conflict_of, p.id);
});

test('borrado lógico aparece en pull con deleted_at; repetir es inocuo', async () => {
  const p = poi(); await pushOne(up(p));
  const r = await pushOne({ op_id: randomUUID(), type: 'delete', id: p.id, base_version: 1 });
  assert.equal(r.status, 'accepted');
  const got = (await pullAll()).pois.find((x) => x.id === p.id);
  assert.ok(got.deleted_at);
  const again = await pushOne({ op_id: randomUUID(), type: 'delete', id: p.id, base_version: 2 });
  assert.equal(again.status, 'accepted');
});

test('validación: rechaza coordenadas, nombre y UUID malos sin registrar la op', async () => {
  for (const bad of [poi({ latitude: 95 }), poi({ name: '  ' }), { ...poi(), id: 'no-uuid' }]) {
    assert.equal((await pushOne(up(bad))).status, 'rejected');
  }
});

test('cursor: pull desde el último seq no devuelve nada; paginación', async () => {
  for (let i = 0; i < 5; i++) await pushOne(up(poi({ name: `P${i}` })));
  const first = await (await fetch(`${A}/sync/pull?cursor=0&limit=2`, { headers: H })).json();
  assert.equal(first.pois.length, 2); assert.equal(first.has_more, true);
  const full = await pullAll();
  const empty = await pullAll(full.cursor);
  assert.equal(empty.pois.length, 0); assert.equal(empty.has_more, false);
  const seqs = full.pois.map((x) => x.seq); assert.deepEqual(seqs, [...seqs].sort((a, b) => a - b));
});

test('lote máximo y ack', async () => {
  const many = Array.from({ length: 101 }, () => up(poi()));
  assert.equal((await post('/sync/push', { ops: many })).status, 413);
  const r = await post('/sync/ack', { cursor: 3, device: 'mac' });
  assert.equal((await r.json()).cursor, 3);
});

test('adjuntos: subir (reintento seguro), citar, descargar; tipo y tamaño controlados', async () => {
  const p = poi(), att = randomUUID();
  const body = Buffer.from('fakejpegbytes');
  const put = () => fetch(`${A}/attachments?poi=${p.id}&att=${att}`, { method: 'PUT', headers: { authorization: H.authorization, 'content-type': 'image/jpeg' }, body });
  const r1 = await put(); assert.equal(r1.status, 201);
  const { key } = await r1.json(); assert.equal(key, `poi/${p.id}/${att}.jpg`);
  assert.equal((await put()).status, 201); // reintento
  const r = await pushOne(up({ ...p, attachments: [{ key, type: 'image/jpeg', bytes: body.length }, { key: 'poi/otro/x.jpg', type: 'image/jpeg', bytes: 1 }] }));
  assert.equal(r.status, 'accepted');
  const got = (await pullAll()).pois.find((x) => x.id === p.id);
  assert.equal(got.attachments.length, 1); // la clave ajena se descarta
  const dl = await fetch(`${A}/attachments/${encodeURIComponent(key)}`, { headers: H });
  assert.equal(dl.status, 200); assert.equal(Buffer.from(await dl.arrayBuffer()).toString(), 'fakejpegbytes');
  assert.equal((await fetch(`${A}/attachments/${encodeURIComponent(key)}`)).status, 401);
  const bad = await fetch(`${A}/attachments?poi=${p.id}`, { method: 'PUT', headers: { authorization: H.authorization, 'content-type': 'text/html' }, body: '<x>' });
  assert.equal(bad.status, 415);
});

test('la PWA se sirve desde el mismo Worker', async () => {
  const r = await fetch(`${BASE}/index.html`);
  assert.equal(r.status, 200); assert.match(await r.text(), /Light Studio PDI/);
});

// ---------- Passkeys (Face ID) con un autenticador simulado ----------
import { generateKeyPairSync, createHash, sign, randomBytes } from 'node:crypto';
const b64u = (b) => Buffer.from(b).toString('base64url');
const ORIGIN = new URL(BASE).origin, RPID = new URL(BASE).hostname;
const authn = () => { const { publicKey, privateKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' }); return { publicKey, privateKey, id: b64u(randomBytes(32)) }; };
const clientData = (type, challenge, origin = ORIGIN) => Buffer.from(JSON.stringify({ type, challenge, origin }));
const authData = (flags = 0x05, rp = RPID) => Buffer.concat([createHash('sha256').update(rp).digest(), Buffer.from([flags]), Buffer.alloc(4)]);
async function registerKey(k) {
  const o = await (await post('/auth/passkey/register/options', {})).json();
  return post('/auth/passkey/register', { id: k.id, publicKey: b64u(k.publicKey.export({ type: 'spki', format: 'der' })), clientDataJSON: b64u(clientData('webauthn.create', o.challenge)) });
}
async function assertion(k, { flags = 0x05, rp = RPID, origin = ORIGIN, challenge, type = 'webauthn.get', key = k.privateKey } = {}) {
  challenge ??= (await (await fetch(`${A}/auth/passkey/login/options`, { method: 'POST' })).json()).challenge;
  const cd = clientData(type, challenge, origin), ad = authData(flags, rp);
  const signature = sign('sha256', Buffer.concat([ad, createHash('sha256').update(cd).digest()]), key); // DER, como hace el móvil
  return { id: k.id, clientDataJSON: b64u(cd), authenticatorData: b64u(ad), signature: b64u(signature), challenge };
}
const login = (a) => post('/auth/passkey/login', a, { 'content-type': 'application/json' });

test('passkey: registrar exige sesión; login con Face ID da un token que funciona', async () => {
  const k = authn();
  const o = await post('/auth/passkey/register/options', {}, { 'content-type': 'application/json' });
  assert.equal(o.status, 401); // sin clave no se registra nada
  assert.equal((await registerKey(k)).status, 201);
  const r = await login(await assertion(k));
  assert.equal(r.status, 200);
  const { token } = await r.json();
  assert.equal((await fetch(`${A}/sync/pull`, { headers: { authorization: `Bearer ${token}` } })).status, 200);
});

test('passkey: se rechazan reto repetido, sin Face ID, otro dominio, otra clave y reto inventado', async () => {
  const k = authn(); await registerKey(k);
  const good = await assertion(k);
  assert.equal((await login(good)).status, 200);
  assert.equal((await login(good)).status, 401);                                   // el reto es de un solo uso
  assert.equal((await login(await assertion(k, { flags: 0x01 }))).status, 401);    // presencia sin verificación (sin Face ID)
  assert.equal((await login(await assertion(k, { rp: 'otro.example' }))).status, 401);
  assert.equal((await login(await assertion(k, { origin: 'https://otro.example' }))).status, 401);
  assert.equal((await login(await assertion(k, { type: 'webauthn.create' }))).status, 401);
  assert.equal((await login(await assertion(k, { key: authn().privateKey }))).status, 401); // firmada con otra clave
  assert.equal((await login(await assertion(k, { challenge: b64u(randomBytes(32)) }))).status, 401);
  assert.equal((await login(await assertion(authn()))).status, 401);               // dispositivo no registrado
});

const route = (o = {}) => ({
  id: randomUUID(), name: 'Cresta', distance_m: 5400.4, ascent_m: 320.6, descent_m: 300,
  points: [[43.1, -2.1, 100.4], [43.2, -2.2], [95, 0], [43.3, -2.3, 150]],
  waypoints: [{ name: 'Fuente', lat: 43.15, lon: -2.15 }, { name: 'Mal', lat: 200, lon: 0 }],
  ...o,
});

test('rutas: se suben, se leen, se sustituyen y se quitan', async () => {
  const r = route();
  assert.equal((await post('/routes', r)).status, 201);
  const list = (await (await fetch(`${A}/routes`, { headers: H })).json()).routes;
  assert.equal(list.find((x) => x.id === r.id).distance_m, 5400);
  const one = await (await fetch(`${A}/routes/${r.id}`, { headers: H })).json();
  assert.deepEqual(one.points, [[43.1, -2.1, 100], [43.2, -2.2], [43.3, -2.3, 150]], 'los puntos imposibles se descartan');
  assert.deepEqual(one.waypoints, [{ name: 'Fuente', lat: 43.15, lon: -2.15 }]);
  // enviarla otra vez la sustituye
  assert.equal((await post('/routes', { ...r, name: 'Cresta 2' })).status, 201);
  assert.equal((await (await fetch(`${A}/routes/${r.id}`, { headers: H })).json()).name, 'Cresta 2');
  assert.equal((await post('/routes/delete', { id: r.id })).status, 200);
  assert.equal((await fetch(`${A}/routes/${r.id}`, { headers: H })).status, 404);
  assert.equal((await post('/routes/delete', { id: r.id })).status, 200, 'quitar una que no está no es un error');
});

test('rutas: piden auth y rechazan lo que no es una ruta', async () => {
  assert.equal((await fetch(`${A}/routes`)).status, 401);
  assert.equal((await post('/routes', { id: 'no-uuid', points: [] })).status, 400);
  assert.equal((await post('/routes', route({ points: [[43.1, -2.1]] }))).status, 400);
  assert.equal((await post('/routes/delete', { id: 'x' })).status, 400);
});
