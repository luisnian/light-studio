# PDI — Light Studio PDI

PWA para apuntar puntos de interés fotográficos con el iPhone, también sin cobertura, y llevarlos a Light Studio.
Worker de Cloudflare con **D1** (registro y cursor) + **R2** (fotos). El Worker sirve también la PWA.

```
worker/   API v1 (src/index.js), esquema D1 (schema.sql), pruebas (test/)
web/      PWA: index.html, app.js, sw.js, manifest, vendor/leaflet
```

Es opcional: Light Studio funciona sin él. Solo hace falta si quieres sincronizar puntos con el móvil. Necesitas una cuenta de Cloudflare (el plan gratuito basta para uso personal) y, si quieres, tu propio dominio (por ejemplo `pdi.example.com`).

## Desplegar (una vez)
```bash
cd worker && npm install
cp wrangler.example.toml wrangler.toml          # tu copia local; no se sube al repositorio
npx wrangler login
npx wrangler d1 create rawstudiopdi            # copia el database_id en wrangler.toml
npx wrangler r2 bucket create rawstudiopdi-files
npx wrangler d1 execute rawstudiopdi --remote --file=schema.sql
npx wrangler secret put API_KEY                # clave larga: la usarán la PWA (1 vez) y Light Studio (llavero)
npx wrangler secret put SESSION_SECRET         # cadena aleatoria (openssl rand -base64 32)
npx wrangler deploy
```
En el iPhone: abrir la URL en Safari → Compartir → *Añadir a pantalla de inicio* → abrir una vez con cobertura y entrar en ⚙︎ con la clave.

## Probar en local
```bash
cd worker
printf 'API_KEY=clave-de-prueba-123\nSESSION_SECRET=secreto-de-prueba\n' > .dev.vars
npx wrangler d1 execute rawstudiopdi --local --persist-to .state --file=schema.sql
npx wrangler dev --local --persist-to .state --port 8799
BASE=http://localhost:8799 npm test            # en otra terminal
```

## API (`/api/v1`, Bearer = clave o token de sesión)
| Ruta | Qué hace |
|---|---|
| `GET /health` | Sin auth. |
| `POST /auth/session` `{key,device}` | Cambia la clave por un token de 30 días. |
| `POST /sync/push` `{ops:[…]}` | ≤100 ops: `upsert` (`op_id`, `base_version`, `poi`) o `delete`. Resultado por op: `accepted`, `duplicate`, `conflict`, `rejected`. |
| `GET /sync/pull?cursor=N&limit=` | PDI con `seq > N` en orden, `cursor` nuevo y `has_more`. Incluye borrados (`deleted_at`). |
| `POST /sync/ack` `{device,cursor}` | Guarda el último cursor del dispositivo. |
| `PUT /attachments?poi=<uuid>&att=<uuid>` | Foto cruda (jpeg/png/webp ≤10 MB) → R2 `poi/<id>/<att>.<ext>`. Repetirla es inocuo. |
| `GET /attachments/<key>` | Descarga (autenticada). |
| `POST /routes` · `GET /routes` · `GET /routes/<id>` · `POST /routes/delete` `{id}` | Rutas que Light Studio envía al móvil (línea, waypoints y cifras; enviar de nuevo con el mismo `id` la sustituye). La PWA las descarga al sincronizar. |
| `POST /auth/passkey/register/options` · `POST /auth/passkey/register` | (con sesión o clave) Registra el Face ID de un dispositivo (WebAuthn ES256). |
| `POST /auth/passkey/login/options` · `POST /auth/passkey/login` | (público) Reto de un solo uso y verificación de la firma (presencia + verificación de usuario); devuelve token de sesión. |

Reglas: altas idempotentes por `op_id`; versión por PDI; **conflicto = el servidor conserva lo suyo y la versión del cliente queda como copia «(conflicto)»**, nunca gana el último sin avisar; borrado lógico.

## Face ID
La PWA se bloquea al abrirla y tras 2 min en segundo plano (`LOCK_AFTER_MS` en `web/app.js`). Se activa una vez en ⚙︎ → *Activar Face ID*. Con cobertura, la firma de Face ID renueva la sesión; sin cobertura, desbloquea en local. «Usar la clave» en la pantalla de bloqueo la desactiva en ese dispositivo. El HTML/JS de la página es público pero no contiene datos; la API sigue exigiendo clave o sesión.
Al actualizar un Worker ya desplegado: `npx wrangler d1 execute rawstudiopdi --remote --file=schema.sql` (crea las tablas nuevas; es repetible) y `npx wrangler deploy`.

## Límites conocidos (v1)
- Mapa: las teselas de OpenStreetMap vistas se guardan (máx. 800), pero **no hay descarga de zonas** para uso sin cobertura; eso exige otro proveedor/licencia (decisión pendiente).
- Solo móvil → Mac en esta versión; la edición desde el Mac llegará con el cliente Rust.
- Los borrados lógicos no se purgan aún (se purgarán cuando todos los dispositivos hagan `ack`).
- Sin probar en iPhone real (Safari/iOS): solo Chrome de escritorio simulando offline.

## Licencias
Código bajo la licencia MIT del repositorio. La PWA incluye [Leaflet](https://leafletjs.com/) 1.9.4 (BSD-2-Clause) en `web/vendor/`.
Mapas: © OpenStreetMap contributors; satélite: Esri, Maxar, Earthstar Geographics.
