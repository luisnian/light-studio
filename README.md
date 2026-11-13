<p align="center"><img src="assets/icon-256.png" width="128" alt="Light Studio"></p>

# Light Studio

Planificador de luz para fotografía de paisaje y noche: Sol, Luna, Vía Láctea, eclipses, sombras del terreno,
horizonte de montañas, alineaciones y puntos de interés sincronizables con el móvil.
*A light planner for landscape and night photography.*

## Qué incluye

- **Planificador**: salida/puesta del Sol y la Luna, horas dorada y azul, ventana de la Vía Láctea, calendario del cielo y vista anual. La zona horaria y el horario de verano se resuelven sin conexión para cualquier lugar del mundo.
- **Mapa**: mapa o satélite, contaminación lumínica, sombras del terreno, horizonte de montañas, herramienta de cámara (campo de visión y profundidad de campo) y cristal esmerilado bajo las tarjetas.
- **Mapa sin conexión**: usa un archivo PMTiles ráster (Ajustes ▸ Mapa) cuando no hay red.
- **Relieve fino (prototipo)**: carpeta de LiDAR de 1 m (.asc, ETRS89 UTM 30N) para afinar las sombras en Gipuzkoa; fuera de la zona cubierta se usa el relieve global.
- **Rutas GPX** (modo «Rutas» del mapa): importa un `.gpx` (Wikiloc, Komoot, Garmin…), dibújalo con salida, llegada y waypoints, mira su perfil de altura, los puntos de interés cercanos, **la luz a lo largo del recorrido** (hora de paso, hora dorada, azul y noche según tu salida y ritmo) y envíalo al móvil para verlo sin cobertura.
- **Horizonte real e índice de salida**: el día solar y lunar calculado sobre la silueta real de las montañas, y un índice que puntúa cada salida (luz, Luna, nubes, brillo del cielo).
- **Vista de montañas en color** con fotografía aérea y terreno más fino, y **sombras sobre todo el mapa** con las horas de sol directo en cada punto.
- **Eclipses con banda besseliana** (trayectoria de la totalidad y de la penumbra), tránsitos de la ISS dibujados en el mapa y búsquedas guardadas sobre la silueta.
- **Mapa sin conexión por regiones**: descarga el mapa de la zona que visitarás; deshacer/rehacer los pines; círculos de tamaño equivalente para componer.
- **Satélites**: pasadas visibles de la ISS y otras estaciones espaciales (órbitas de CelesTrak) y tránsitos por el Sol y la Luna con su duración, en la pestaña Noche.
- **Búsqueda de alineaciones** (Sol/Luna/núcleo galáctico con un punto del terreno) y **eclipses**, validados al minuto con el eclipse total del 2 de agosto de 2027.
- **Cálculos**: exposición, NPF, time-lapse, filtros ND, trazas de estrellas.
- **Planes y puntos de interés** guardados, con exportación a Calendario (macOS) y sincronización con la app web del móvil (PDI).
  La sincronización es opcional: necesita tu propio Worker de Cloudflare, cuyo código y la app web del móvil están en [`pdi/`](pdi/) (ver su README). La dirección (por defecto `https://pdi.example.com`) y la clave se ponen en Ajustes.
- **Modo luz roja** para conservar la visión nocturna en el campo, y opción de reducir el movimiento.
- Interfaz en español e inglés.
- Crate `light-studio-astro`: astronomía propia (teoría lunar de Meeus, ΔT, paralaje topocéntrica, refracción, eclipses), sin dependencias externas.

## Web

[lightstudio.luisnietofoto.com](https://lightstudio.luisnietofoto.com)

## Descargar

Descarga el `.dmg` de la última [versión](https://github.com/luisnian/light-studio/releases/latest), ábrelo y arrastra
Light Studio a Aplicaciones. La app no está notarizada: la primera vez, clic derecho ▸ Abrir.

## Compilar

```sh
cargo run --release -p light-studio
cargo test
```

### .dmg para macOS

```sh
./scripts/make-dmg.sh            # para el chip de este Mac
./scripts/make-dmg.sh universal  # Apple Silicon + Intel
```

Genera `dist/Light-Studio-<versión>.dmg` (firma ad hoc; la primera vez, clic derecho ▸ Abrir).

Requiere Rust 1.90 o superior. Solo para macOS.

## Variables de entorno (opcionales)

`LIGHT_STUDIO_CONFIG_DIR`, `LIGHT_STUDIO_MAP_TILES`, `LIGHT_STUDIO_ELEVATION_URL`, `LIGHT_STUDIO_GEOCODE_URL`,
`LIGHT_STUDIO_TERRAIN_URL`, `LIGHT_STUDIO_OVERPASS_URL`, `LIGHT_STUDIO_PDI_KEY`.

## Servicios y datos de terceros

Light Studio consulta servicios públicos gratuitos, con sus límites de uso: si lo usas mucho, pueden limitarte o
bloquearte. Puedes sustituirlos con las variables de entorno de abajo.

- Mapas: © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors (ODbL), CARTO, y Esri World Imagery para el satélite.
- Contaminación lumínica: atlas de David Lorenz ([djlorenz.github.io/astronomy](https://djlorenz.github.io/astronomy/)).
- Relieve: Terrarium (Mapzen / AWS Open Data) y Open Topo Data; opcionalmente LiDAR de 1 m del b5m (Gipuzkoa) que descargas tú.
- Búsqueda de lugares: Nominatim y Photon; puntos de interés: Overpass.
- Tiempo: Open-Meteo. Órbitas de satélites: [CelesTrak](https://celestrak.org/).
- Localización aproximada por IP (opcional): ipwho.is e ipapi.co, que reciben tu dirección IP.
- Zonas horarias: [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder) (ODbL) a través de `tzf-rs`, y la base de datos IANA.

Los datos conservan las licencias y los términos de sus autores. El icono y el logo son obra original de Luis Nieto.
Detalles en [NOTICE](NOTICE).

## Licencia

MIT © Luis Nieto. Ver [LICENSE](LICENSE).
