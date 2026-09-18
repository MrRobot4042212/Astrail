# PresentMon (FPS / frametime para el overlay)

El overlay de métricas obtiene **FPS, frametime y latencia de presentación** con
[PresentMon](https://github.com/GameTechDev/PresentMon) (Intel/Microsoft), que usa
**ETW** (event tracing del kernel) — **sin inyección de DLL**, por lo que es seguro
frente a anti-cheats. Por licencia/peso, el binario **no se incluye** en el repo: hay
que añadirlo aquí.

## Cómo habilitar FPS

1. **Descarga `PresentMon.exe`** de las releases oficiales:
   https://github.com/GameTechDev/PresentMon/releases
   (la versión CLI de 64 bits; renómbrala a `PresentMon.exe` si trae sufijo de versión).

2. **Colócala en esta carpeta**: `src-tauri/binaries/PresentMon.exe`.
   - Para **desarrollo** (`npm run app`) con esto basta: el controlador la busca aquí,
     junto al ejecutable y en el `resource_dir`.

3. **Para el instalador** (`npm run app:build`), empaquétala como recurso añadiendo a
   `src-tauri/tauri.conf.json` dentro de `"bundle"`:
   ```json
   "resources": ["binaries/PresentMon.exe"]
   ```
   (No la añadas hasta tener el `.exe` aquí, o el build de release fallará.)

## Requisito de permisos

ETW en tiempo real **exige ejecutar Astrail como administrador**. Sin elevación,
PresentMon no puede abrir la sesión ETW y el overlay simplemente **omite FPS/frametime**
(el resto de métricas —GPU/CPU/temperaturas— funcionan sin admin). No forzamos elevación
de toda la app a propósito; quien quiera FPS ejecuta Astrail como admin.

## Detalles de integración
-
`src-tauri/src/presentmon.rs` lanza `PresentMon.exe -process_id <pid> -output_stdout
-stop_existing_session -no_top -terminate_on_proc_exit`, parsea la columna
`*BetweenPresents` (frametime en ms) del CSV en stdout y mantiene una ventana móvil de
~1 s para derivar FPS y frametime medio. Apunta al PID del juego en primer plano que
publica el watcher de playtime.
