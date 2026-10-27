<p align="center"><img src="assets/icon-256.png" width="128" alt="Light Studio"></p>

# Light Studio

Planificador de luz para fotografía de paisaje y noche: Sol, Luna, Vía Láctea, eclipses, sombras del terreno,
horizonte de montañas, alineaciones y puntos de interés sincronizables con el móvil.
*A light planner for landscape and night photography.*

## Qué incluye

- **Planificador**: salida/puesta del Sol y la Luna, horas dorada y azul, ventana de la Vía Láctea, calendario del cielo y vista anual.
- **Mapa**: mapa o satélite, contaminación lumínica, sombras, horizonte, herramienta de cámara (campo de visión y profundidad de campo).
- **Búsqueda de alineaciones** (Sol/Luna/núcleo galáctico con un punto del terreno) y eclipses.
- **Cálculos**: exposición, NPF, time-lapse, filtros ND, trazas de estrellas.
- **Planes y puntos de interés** guardados, con exportación a Calendario (macOS) y sincronización con la app web del móvil (PDI).
- Interfaz en español e inglés.
- Crate `light-studio-astro`: astronomía propia (efemérides, tiempo, refracción, eclipses), sin dependencias externas.

## Compilar

```sh
cargo run --release -p light-studio
cargo test
```

Requiere Rust 1.90 o superior. Funciona en macOS, Linux y Windows (wgpu).

## Variables de entorno (opcionales)

`LIGHT_STUDIO_CONFIG_DIR`, `LIGHT_STUDIO_MAP_TILES`, `LIGHT_STUDIO_ELEVATION_URL`, `LIGHT_STUDIO_GEOCODE_URL`,
`LIGHT_STUDIO_TERRAIN_URL`, `LIGHT_STUDIO_OVERPASS_URL`, `LIGHT_STUDIO_PDI_KEY`.

## Licencia

MIT © Luis Nieto. Ver [LICENSE](LICENSE).
