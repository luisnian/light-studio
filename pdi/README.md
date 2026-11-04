# PDI — app web del móvil

PWA para anotar puntos de interés fotográficos (PDI) desde el móvil, también sin cobertura, y sincronizarlos con
Light Studio. Versión 0.8.1. HTML, CSS y JavaScript sin dependencias de compilación; el mapa usa
[Leaflet](https://leafletjs.com/) 1.9.4 (BSD-2-Clause), incluido en `web/vendor/`.

- Funciona primero en local (IndexedDB) y sincroniza cuando hay red.
- Se instala en la pantalla de inicio (service worker + manifiesto) y puede bloquearse con Face ID / passkey.
- Mapa base: OpenStreetMap o satélite de Esri (con sus atribuciones).

## Qué hace falta para usarla

La app es solo el cliente: guarda y sincroniza contra un **Worker de Cloudflare** (con una base D1 para los puntos y un
bucket R2 para las fotos) que **no está incluido en este repositorio**. La app web se sirve desde el mismo dominio que el
Worker, porque llama a rutas relativas `/api/v1/...`.

Para ponerlo en marcha necesitas:

1. Un Worker que sirva `web/` como archivos estáticos y ofrezca la API de abajo.
2. Una base D1 y un bucket R2 propios, y una clave de acceso (guárdala como secreto del Worker, nunca en el código).
3. Tu dominio, por ejemplo `pdi.example.com`. En Light Studio ▸ Ajustes ▸ PDI pones esa dirección y la clave.

No publiques tu `wrangler.toml` real: lleva el identificador de tu cuenta y los de D1/R2. Usa una copia
`wrangler.example.toml` con valores de ejemplo.

## API que espera el cliente (`/api/v1`)

| Ruta | Uso |
|---|---|
| `GET /health` | Comprobación de que el Worker responde |
| `POST /auth/session` | Cambia la clave del dispositivo por un token (`{key, device}`) |
| `POST /auth/passkey/register/options`, `POST /auth/passkey/register` | Alta de passkey (Face ID) |
| `POST /auth/passkey/login/options`, `POST /auth/passkey/login` | Entrada con passkey |
| `POST /pair`, `POST /pair/claim` | Emparejar un dispositivo con un código de 8 caracteres (`XXXX-XXXX`) |
| `GET /devices`, `POST /devices/{id}/revoke` | Listar y revocar dispositivos |
| `POST /sync/push`, `GET /sync/pull?cursor=`, `POST /sync/ack` | Sincronización de cambios (por cursor, con borrados) |
| `PUT /attachments?poi=&att=`, `GET /attachments/{key}` | Fotos de cada punto |
| `GET /manifest?pair=` | Manifiesto que lleva el código de emparejamiento al instalar la app |

Las rutas protegidas usan `Authorization: Bearer <token>`. El detalle de formato lo definen `web/app.js` (cliente móvil)
y `crates/app/src/services/pdi_sync.rs` (cliente de Light Studio).
