# Medición de rendimiento

Cada fase del plan de optimización tiene un criterio de salida numérico. Este
directorio contiene la herramienta que produce esos números y los resultados
guardados, para que "va más rápido" nunca sea una impresión.

## Capturar

1. Arranca Meteor (`npm run app` o el instalado) y **cierra la ventana** para que
   quede en la bandeja. Sin juego abierto.
2. Deja el equipo tranquilo y ejecuta:

```powershell
powershell -File docs\perf\capture.ps1 -Label baseline -Minutes 10
```

Escribe `docs/perf/<label>.json`. Tras cada fase, repite con otra etiqueta y
compara:

```powershell
powershell -File docs\perf\capture.ps1 -Label fase3 -Minutes 10 -Compare docs\perf\baseline.json
```

## Qué mide (y por qué)

| Campo | Qué es | Por qué importa |
|---|---|---|
| `cpu_percent_mean` | `\Process(meteor)\% Processor Time` normalizado por núcleo | Trabajo de fondo con la app en bandeja |
| `context_switches_s` | Cambios de contexto/s de los hilos de `meteor` | Cuántas veces despierta: el watcher, el sampler y el controlador de cputemp tenían temporizadores fijos |
| `meteor_private_mb` / `webview_workingset_mb` | Memoria privada del proceso y del árbol WebView2 | El WebView2 sigue residente al ocultar la ventana |
| `files_written` | Ficheros de `%APPDATA%\com.alfonso.meteor` cuya fecha cambió durante la ventana | En reposo debe ser **0**; el watcher escribía `active_sessions.json` cada 5 s |
| `nvml_loaded` / `sidecar_running` | Si está cargada la DLL de NVML y si corre el sidecar `cputemp` | Las métricas de GPU se cargaban al arrancar aunque el overlay estuviese apagado |
| `caches.covers_mb` | Tamaño de la caché de portadas | Crece sin límite hasta la Fase 4 |

## Medidas que no captura el script

- **HUD en juego**: `METEOR_OVERLAY_DEBUG=1` antes de arrancar; comprueba en el
  log que el modo de composición sigue siendo `OVERLAY` (si baja a `COMPOSED`,
  el overlay le está costando FPS al juego y el cambio se revierte).
- **`get_library`**: `METEOR_PERF=1` antes de arrancar; imprime líneas
  `perf get_library <ms>`, `perf xbox::scan <ms>` y `perf art::resolve <ms>` en
  stderr (ver `src-tauri/src/perf.rs`).
- **Rejilla de la biblioteca**: `NEXT_PUBLIC_MOCK_LIBRARY=500 npm run dev` genera
  500 juegos sintéticos sin tocar tus datos; mide con el profiler de React
  DevTools (commits por pase de portadas y commit más largo).
- **Tamaños de build**: `cargo build --release --timings`, `npm run build`.

## Resultados

| Fichero | Cuándo | Nota |
|---|---|---|
| _(pendiente)_ | | Ejecuta la captura `baseline` antes de la Fase 3 |
